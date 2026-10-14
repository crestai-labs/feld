use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use serde_json::json;

use crate::store::{
    LaunchUpdate, ObservationUpdate, PendingRestart, RecordFilter, RequestIdentity,
    ScheduleDeclaration, SessionWriteError, Store,
};
use feld_core::home::FeldHome;
use feld_core::model::{ActorKind, LaunchSpec, ObservationKind, RecordKind, Requester, Schedule};
use feld_core::plugin::protocol::{
    ControlResponse, IdentityRequest, IdentityResponse, MatchRequest, MatchResponse,
    ObservationReport, ObserveRequest, ObserveResponse, ObserveResult, PlaceRequest, PlaceResponse,
    PlacementRequest, PrepareRequest, PrepareResponse, ProviderAttachRequest,
    ProviderAttachResponse, ProviderLaunchRequest, ProviderLaunchResponse, ResolveRequest,
    ResolveResponse, RestoreRequest, RestoreResponse, RuntimeLaunchRequest, RuntimeLaunchResponse,
    SelfResponse, StopRequest, TranscriptRequest, TranscriptResponse,
};
use feld_core::plugin::{ExecutionProvider, Manifest, PluginKind, Plugins, RuntimeAdapter};

use super::start::StartSource;
use super::{ControlError, ScheduleTickResult, SessionController};

#[tokio::test]
async fn shutdown_while_tick_waits_for_control_lock_cancels_before_admission() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
    home.create().unwrap();
    let store = Arc::new(Store::open(&home.store_path()).unwrap());
    let launch = LaunchSpec {
        runtime: None,
        runtime_settings: None,
        trust_workdir: None,
        command: Some(vec!["true".to_owned()]),
        workdir: Some("/tmp".to_owned()),
        env: None,
        provider: Some("tmux".to_owned()),
        node: Some("local".to_owned()),
        provider_options: Some(serde_json::json!({
            "socket": "feld-shutdown-test",
            "unit": "session"
        })),
    };
    store
        .create_actor(
            "default",
            "job",
            ActorKind::Job,
            &launch,
            Some(&ScheduleDeclaration {
                cron: "* * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
            None,
        )
        .unwrap();
    let schedule = store.get_schedule("default", "job").unwrap().unwrap();
    let controller = Arc::new(SessionController::new(
        Arc::clone(&store),
        home,
        "http://127.0.0.1:1".to_owned(),
    ));
    let held = controller.operation.lock().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let waiting = {
        let controller = Arc::clone(&controller);
        tokio::spawn(async move {
            controller
                .schedule_tick(
                    "default",
                    "job",
                    &schedule,
                    "2026-09-16T04:00:00Z",
                    "2026-09-16T04:00:00.001Z",
                    &shutdown_rx,
                )
                .await
        })
    };
    tokio::task::yield_now().await;
    shutdown_tx.send(true).unwrap();
    drop(held);

    assert!(matches!(
        waiting.await.unwrap().unwrap(),
        ScheduleTickResult::Cancelled
    ));
    assert!(store.list_sessions(Some("default")).unwrap().is_empty());
    assert!(
        store
            .list_records(&RecordFilter {
                kind: Some(RecordKind::ScheduleSkipped),
                ..RecordFilter::default()
            })
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_start_whose_launch_changed_during_place_places_again_with_the_new_launch() {
    let fixture = Fixture::new(program_launch("/tmp"));
    let store = Arc::clone(&fixture.store);
    fixture.provider.on_place(move |place| {
        if place == 0 {
            change_launch(&store, "/tmp", "/var");
        }
    });

    let started = fixture.start().await.unwrap();

    assert_eq!(fixture.provider.places.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.provider.launches.load(Ordering::SeqCst), 1);
    assert_eq!(started.session.placement.workdir, "/var");
    assert!(started.session.end.is_none());
    assert_eq!(fixture.sessions().len(), 1);
}

#[tokio::test]
async fn a_start_whose_launch_keeps_changing_gives_up_without_admitting() {
    let fixture = Fixture::new(program_launch("/tmp"));
    let store = Arc::clone(&fixture.store);
    fixture.provider.on_place(move |place| {
        let (from, to) = if place % 2 == 0 {
            ("/tmp", "/var")
        } else {
            ("/var", "/tmp")
        };
        change_launch(&store, from, to);
    });

    let refused = fixture.start().await.unwrap_err();

    assert!(matches!(refused, ControlError::LaunchKeptChanging));
    assert_eq!(fixture.provider.places.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.provider.launches.load(Ordering::SeqCst), 0);
    assert!(fixture.sessions().is_empty());
}

#[tokio::test]
async fn a_start_that_admission_refuses_is_never_placed() {
    let fixture = Fixture::new(program_launch("/tmp"));
    fixture.start().await.unwrap();

    let refused = fixture.start().await.unwrap_err();

    assert!(matches!(
        refused,
        ControlError::Store(SessionWriteError::Admission { ref observation, .. })
            if observation == "running"
    ));
    assert_eq!(fixture.provider.places.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.provider.launches.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.sessions().len(), 1);
}

#[tokio::test]
async fn a_tick_that_admission_does_not_take_is_never_placed() {
    let fixture = Fixture::scheduled(program_launch("/tmp"));
    let schedule = fixture
        .store
        .get_schedule("default", "actor")
        .unwrap()
        .unwrap();

    // A tick already recorded, here by a start that left nothing behind and closed.
    fixture
        .provider
        .creates_nothing
        .store(true, Ordering::SeqCst);
    let first = "2026-10-07T00:00:00Z";
    assert!(matches!(
        fixture.tick(&schedule, first).await,
        ScheduleTickResult::Started(_)
    ));
    assert!(matches!(
        fixture.tick(&schedule, first).await,
        ScheduleTickResult::Already
    ));
    assert_eq!(fixture.places(), 1);

    // A tick of a Schedule the Actor no longer has.
    let mut changed = schedule.clone();
    changed.cron = "0 0 * * *".to_owned();
    assert!(matches!(
        fixture.tick(&changed, "2026-10-07T00:01:00Z").await,
        ScheduleTickResult::Stale
    ));
    assert_eq!(fixture.places(), 1);

    // A tick that meets a running current Session.
    fixture
        .provider
        .creates_nothing
        .store(false, Ordering::SeqCst);
    assert!(matches!(
        fixture.tick(&schedule, "2026-10-07T00:02:00Z").await,
        ScheduleTickResult::Started(_)
    ));
    assert!(matches!(
        fixture.tick(&schedule, "2026-10-07T00:03:00Z").await,
        ScheduleTickResult::Skipped(_)
    ));
    assert_eq!(fixture.places(), 2);
    assert_eq!(fixture.provider.launches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_restart_that_no_longer_applies_is_never_placed() {
    let fixture = Fixture::new(program_launch("/tmp"));
    let started = fixture.start().await.unwrap();

    // No restart was accepted for this Session, so there is nothing to start.
    let restart = PendingRestart {
        session_id: started.session.id.clone(),
        workspace: "default".to_owned(),
        actor: "actor".to_owned(),
        spawner: Requester::Admin,
        spawner_session_id: None,
    };
    let result = fixture
        .controller
        .start_locked("default", "actor", StartSource::Restart(&restart))
        .unwrap();

    assert!(matches!(result, ScheduleTickResult::Stale));
    assert_eq!(fixture.places(), 1);
    assert_eq!(fixture.sessions().len(), 1);
}

#[tokio::test]
async fn a_runtime_that_cannot_be_prepared_records_a_start_that_never_began() {
    let mut launch = program_launch("/tmp");
    launch.command = None;
    launch.runtime = Some("stub-runtime".to_owned());
    let fixture = Fixture::with_kind(ActorKind::Agent, launch);

    let started = fixture.start().await.unwrap();

    assert_eq!(fixture.provider.launches.load(Ordering::SeqCst), 0);
    let session = started.session;
    assert!(session.end.is_some());
    assert!(session.lifecycle.start_failed.is_some());
    let observation = session.observation.unwrap();
    assert_eq!(observation.kind, ObservationKind::Gone);
    assert_eq!(
        observation.detail.as_deref(),
        Some("the Stub Runtime could not be prepared: no Runtime here")
    );
}

#[tokio::test]
async fn a_provider_that_created_nothing_records_a_start_that_never_began() {
    let fixture = Fixture::new(program_launch("/tmp"));
    fixture
        .provider
        .creates_nothing
        .store(true, Ordering::SeqCst);

    let started = fixture.start().await.unwrap();

    let session = started.session;
    assert!(session.end.is_some());
    assert!(session.lifecycle.start_failed.is_some());
    assert_eq!(session.observation.unwrap().kind, ObservationKind::Gone);
}

#[tokio::test]
async fn an_observe_answer_of_another_length_records_nothing() {
    let fixture = Fixture::new(program_launch("/tmp"));
    let started = fixture.start().await.unwrap();
    fixture
        .provider
        .observes_nothing
        .store(true, Ordering::SeqCst);

    let refused = fixture
        .controller
        .observe(&started.session.id)
        .await
        .unwrap_err();

    assert!(matches!(
        refused,
        ControlError::ObserveMismatch { ref provider, answered: 0 } if provider == "stub"
    ));
    assert_eq!(refused.to_string(), "stub answered observe with nothing");
    let session = fixture.sessions().remove(0);
    assert!(session.end.is_none());
    assert_eq!(session.observation, started.session.observation);
}

#[tokio::test]
async fn a_provider_whose_observe_fails_leaves_the_others_and_the_restarts_to_go_on() {
    let fixture = Fixture::new(program_launch("/tmp"));
    let stopped = fixture.stop_for_restart().await;
    let broken = fixture.start_another("broken", "stub").await;
    let other = fixture.start_another("other", "other").await;
    fixture
        .provider
        .observes_nothing
        .store(true, Ordering::SeqCst);

    let problems = fixture.controller.recover().await.unwrap();

    assert_eq!(
        problems,
        ["the Sessions of Provider 'stub' were not observed: stub answered observe with nothing"]
    );
    assert_eq!(fixture.other.observes.load(Ordering::SeqCst), 1);
    let open: Vec<_> = fixture
        .sessions()
        .into_iter()
        .filter(|session| session.end.is_none())
        .map(|session| session.id)
        .collect();
    assert!(open.contains(&broken.id) && open.contains(&other.id));
    assert!(fixture.restarted(&stopped));
}

#[tokio::test]
async fn a_session_whose_observation_cannot_be_recorded_leaves_the_others_to_go_on() {
    let fixture = Fixture::new(program_launch("/tmp"));
    let stopped = fixture.stop_for_restart().await;
    let refused = fixture.start_another("refused", "stub").await;
    // Starts a millisecond apart are read in that order, so the refused Session comes first.
    std::thread::sleep(Duration::from_millis(5));
    let recorded = fixture.start_another("recorded", "stub").await;
    let order: Vec<_> = fixture
        .store
        .list_open_sessions()
        .unwrap()
        .into_iter()
        .map(|session| session.id)
        .collect();
    assert_eq!(order, [refused.id.clone(), recorded.id.clone()]);
    // The Store refuses every write of the first Session's observation.
    rusqlite::Connection::open(fixture._home.path().join("admin/feld.db"))
        .unwrap()
        .execute_batch(&format!(
            "CREATE TRIGGER refuse_observation BEFORE UPDATE OF observation_at ON sessions
             WHEN OLD.id = '{}' BEGIN SELECT RAISE(ABORT, 'refused'); END;",
            refused.id
        ))
        .unwrap();
    std::thread::sleep(Duration::from_millis(5));

    let problems = fixture.controller.recover().await.unwrap();

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with(&format!("Session '{}' was not observed: ", refused.id)),
        "{problems:?}"
    );
    let sessions = fixture.sessions();
    let read = |id: &str| sessions.iter().find(|session| session.id == id).unwrap();
    assert_eq!(read(&refused.id).observation, refused.observation);
    assert_ne!(
        read(&recorded.id).observation.as_ref().unwrap().at,
        recorded.observation.as_ref().unwrap().at
    );
    assert!(fixture.restarted(&stopped));
}

#[tokio::test]
async fn a_provider_that_blocks_leaves_the_runtime_free() {
    let fixture = Fixture::new(program_launch("/tmp"));
    let started = fixture.start().await.unwrap();
    fixture.provider.holds_observe.store(true, Ordering::SeqCst);

    // The test runs on one thread, so the release below only happens while `observe` runs
    // when the call leaves that thread.
    let observing = {
        let controller = Arc::clone(&fixture.controller);
        let id = started.session.id.clone();
        tokio::spawn(async move { controller.observe(&id).await })
    };
    while !fixture.provider.observing.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    fixture.provider.released.store(true, Ordering::SeqCst);

    let session = observing.await.unwrap().unwrap();
    assert_eq!(session.observation.unwrap().kind, ObservationKind::Running);
}

/// A Store with one Actor that launches through [`StubProvider`], and the controller that
/// starts it.
struct Fixture {
    _home: tempfile::TempDir,
    store: Arc<Store>,
    provider: Arc<StubProvider>,
    /// The Provider named `other`.
    other: Arc<StubProvider>,
    controller: Arc<SessionController>,
}

impl Fixture {
    fn new(launch: LaunchSpec) -> Self {
        Self::with_kind(ActorKind::Job, launch)
    }

    fn with_kind(kind: ActorKind, launch: LaunchSpec) -> Self {
        Self::with_schedule(kind, launch, None)
    }

    fn scheduled(launch: LaunchSpec) -> Self {
        Self::with_schedule(
            ActorKind::Job,
            launch,
            Some(&ScheduleDeclaration {
                cron: "* * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
        )
    }

    fn with_schedule(
        kind: ActorKind,
        launch: LaunchSpec,
        schedule: Option<&ScheduleDeclaration>,
    ) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
        home.create().unwrap();
        let store = Arc::new(Store::open(&home.store_path()).unwrap());
        store
            .create_actor("default", "actor", kind, &launch, schedule, None)
            .unwrap();
        let provider = Arc::new(StubProvider::default());
        let other = Arc::new(StubProvider::default());
        let plugins = Arc::new(StubPlugins {
            provider: Arc::clone(&provider),
            other: Arc::clone(&other),
            runtime: Arc::new(StubRuntime {
                manifest: manifest("stub-runtime", PluginKind::Runtime, "Stub"),
            }),
        });
        let controller = Arc::new(SessionController::with_collector_state(
            Arc::clone(&store),
            plugins,
            home,
            "http://127.0.0.1:1".to_owned(),
            Arc::default(),
        ));
        Self {
            _home: temporary,
            store,
            provider,
            other,
            controller,
        }
    }

    /// Starts the Actor's Session and has it close after a restart was accepted, so that
    /// the restart waits for its next generation.
    async fn stop_for_restart(&self) -> feld_core::model::Session {
        let stopped = self.start().await.unwrap().session;
        self.store
            .accept_restart(
                "default",
                "actor",
                RequestIdentity {
                    requester: &Requester::Admin,
                    session_id: None,
                },
            )
            .unwrap();
        self.store
            .record_observation(
                &stopped.id,
                &ObservationUpdate {
                    kind: ObservationKind::Gone,
                    at: feld_core::time::now(),
                    detail: None,
                    exit_code: Some(0),
                },
                None,
            )
            .unwrap();
        stopped
    }

    /// Whether a later generation of `stopped`'s Actor is open.
    fn restarted(&self, stopped: &feld_core::model::Session) -> bool {
        self.sessions().iter().any(|session| {
            session.end.is_none()
                && session.actor_id == stopped.actor_id
                && session.id != stopped.id
        })
    }

    /// Declares another `program` Job placed by `provider` and starts it.
    async fn start_another(&self, actor: &str, provider: &str) -> feld_core::model::Session {
        let mut launch = program_launch("/tmp");
        launch.provider = Some(provider.to_owned());
        self.store
            .create_actor("default", actor, ActorKind::Job, &launch, None, None)
            .unwrap();
        self.controller
            .start("default", actor, &Requester::Admin, None)
            .await
            .unwrap()
            .session
    }

    async fn start(&self) -> Result<super::StartedSession, ControlError> {
        self.controller
            .start("default", "actor", &Requester::Admin, None)
            .await
    }

    async fn tick(&self, schedule: &Schedule, planned: &str) -> ScheduleTickResult {
        let (_shutdown, shutdown) = tokio::sync::watch::channel(false);
        self.controller
            .schedule_tick("default", "actor", schedule, planned, planned, &shutdown)
            .await
            .unwrap()
    }

    fn places(&self) -> usize {
        self.provider.places.load(Ordering::SeqCst)
    }

    fn sessions(&self) -> Vec<feld_core::model::Session> {
        self.store.list_sessions(Some("default")).unwrap()
    }
}

fn program_launch(workdir: &str) -> LaunchSpec {
    LaunchSpec {
        runtime: None,
        runtime_settings: None,
        trust_workdir: None,
        command: Some(vec!["true".to_owned()]),
        workdir: Some(workdir.to_owned()),
        env: None,
        provider: Some("stub".to_owned()),
        node: Some("local".to_owned()),
        provider_options: None,
    }
}

/// Another update of the Actor's launchSpec commits: the one at `from` becomes `to`.
fn change_launch(store: &Store, from: &str, to: &str) {
    store
        .update_actor(
            "default",
            "actor",
            Some(LaunchUpdate {
                expected: Some(&program_launch(from)),
                resolved: &program_launch(to),
            }),
            None,
            None,
            "UTC",
        )
        .unwrap();
}

fn manifest(name: &str, kind: PluginKind, label: &str) -> Manifest {
    Manifest {
        name: name.to_owned(),
        kind,
        label: label.to_owned(),
        stop_input: None,
        owned_settings: Vec::new(),
        extra: Default::default(),
    }
}

struct StubPlugins {
    provider: Arc<StubProvider>,
    other: Arc<StubProvider>,
    runtime: Arc<StubRuntime>,
}

impl Plugins for StubPlugins {
    fn runtime(&self, name: &str) -> Option<Arc<dyn RuntimeAdapter>> {
        (name == "stub-runtime").then(|| Arc::clone(&self.runtime) as Arc<dyn RuntimeAdapter>)
    }

    fn provider(&self, name: &str) -> Option<Arc<dyn ExecutionProvider>> {
        match name {
            "stub" => Some(Arc::clone(&self.provider) as Arc<dyn ExecutionProvider>),
            "other" => Some(Arc::clone(&self.other) as Arc<dyn ExecutionProvider>),
            _ => None,
        }
    }
}

/// An Execution Provider whose executions run without end, counting the calls it answers.
struct StubProvider {
    manifest: Manifest,
    /// Runs inside each `place`, with how many came before it.
    on_place: Mutex<Box<dyn Fn(usize) + Send>>,
    places: AtomicUsize,
    launches: AtomicUsize,
    observes: AtomicUsize,
    /// `launch` answers `gone` with no Placement.
    creates_nothing: AtomicBool,
    /// `observe` answers no results.
    observes_nothing: AtomicBool,
    /// `observe` blocks until [`StubProvider::release`] or its deadline.
    holds_observe: AtomicBool,
    observing: AtomicBool,
    released: AtomicBool,
}

impl Default for StubProvider {
    fn default() -> Self {
        Self {
            manifest: manifest("stub", PluginKind::Provider, "Stub"),
            on_place: Mutex::new(Box::new(|_| {})),
            places: AtomicUsize::new(0),
            launches: AtomicUsize::new(0),
            observes: AtomicUsize::new(0),
            creates_nothing: AtomicBool::new(false),
            observes_nothing: AtomicBool::new(false),
            holds_observe: AtomicBool::new(false),
            observing: AtomicBool::new(false),
            released: AtomicBool::new(false),
        }
    }
}

impl StubProvider {
    fn on_place(&self, action: impl Fn(usize) + Send + 'static) {
        *self.on_place.lock().unwrap() = Box::new(action);
    }
}

fn running() -> ObservationReport {
    ObservationReport {
        kind: ObservationKind::Running,
        exit_code: None,
        detail: None,
    }
}

impl ExecutionProvider for StubProvider {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn current(&self) -> Result<SelfResponse> {
        unimplemented!("a start never asks where it runs")
    }

    fn resolve(&self, _: &ResolveRequest) -> Result<ResolveResponse> {
        unimplemented!("a start never resolves")
    }

    fn place(&self, _: &PlaceRequest) -> Result<PlaceResponse> {
        let place = self.places.fetch_add(1, Ordering::SeqCst);
        (self.on_place.lock().unwrap())(place);
        Ok(PlaceResponse {
            placement: json!({ "summary": "stub" }),
            summary: "stub".to_owned(),
        })
    }

    fn launch(&self, _: &ProviderLaunchRequest) -> Result<ProviderLaunchResponse> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        if self.creates_nothing.load(Ordering::SeqCst) {
            return Ok(ProviderLaunchResponse {
                placement: None,
                observation: ObservationReport::new(ObservationKind::Gone, "nothing was created"),
                summary: String::new(),
                attach_command: None,
            });
        }
        Ok(ProviderLaunchResponse {
            placement: Some(json!({ "summary": "stub", "incarnation": 1 })),
            observation: running(),
            summary: "stub".to_owned(),
            attach_command: None,
        })
    }

    fn observe(&self, request: &ObserveRequest) -> Result<ObserveResponse> {
        self.observes.fetch_add(1, Ordering::SeqCst);
        if self.holds_observe.load(Ordering::SeqCst) {
            self.observing.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.released.load(Ordering::SeqCst) {
                if Instant::now() >= deadline {
                    return Err(anyhow!("nothing released the observe"));
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        let answered = if self.observes_nothing.load(Ordering::SeqCst) {
            0
        } else {
            request.placements.len()
        };
        Ok(ObserveResponse {
            results: (0..answered)
                .map(|_| ObserveResult {
                    observation: running(),
                    summary: "stub".to_owned(),
                    attach_command: None,
                })
                .collect(),
        })
    }

    fn stop(&self, _: &StopRequest) -> Result<ControlResponse> {
        unimplemented!("a start never stops")
    }

    fn kill(&self, _: &PlacementRequest) -> Result<ControlResponse> {
        unimplemented!("a start never kills")
    }

    fn cleanup(&self, _: &PlacementRequest) -> Result<()> {
        Ok(())
    }

    fn attach(&self, _: &ProviderAttachRequest) -> Result<ProviderAttachResponse> {
        unimplemented!("a start never attaches")
    }
}

/// A Runtime Adapter that cannot prepare a launch.
struct StubRuntime {
    manifest: Manifest,
}

impl RuntimeAdapter for StubRuntime {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn launch(&self, _: &RuntimeLaunchRequest) -> Result<RuntimeLaunchResponse> {
        Err(anyhow!("no Runtime here"))
    }

    fn match_process(&self, _: &MatchRequest) -> Result<MatchResponse> {
        unimplemented!("a start never matches")
    }

    fn identity(&self, _: &IdentityRequest) -> Result<IdentityResponse> {
        unimplemented!("a start never asks for an identity")
    }

    fn prepare(&self, _: &PrepareRequest) -> Result<PrepareResponse> {
        unimplemented!("a start never prepares")
    }

    fn restore(&self, _: &RestoreRequest) -> Result<RestoreResponse> {
        unimplemented!("a start never restores")
    }

    fn transcript(&self, _: &TranscriptRequest) -> Result<TranscriptResponse> {
        unimplemented!("a start never reads a transcript")
    }
}
