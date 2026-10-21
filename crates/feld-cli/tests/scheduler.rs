use std::fs;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, SecondsFormat, Timelike, Utc};
use feld_core::cron::{last_run_at_or_before, next_run_after};
use feld_core::home::FeldHome;
use feld_core::model::{
    ActorKind, EndKind, Execution, LaunchSpec, ObservationKind, Placement, Record, RecordKind,
    Requester, Schedule, StartKind,
};
use feld_daemon::control::SessionController;
use feld_daemon::scheduler::{Clock, Scheduler, SystemClock, Watch};
use feld_daemon::service::ActorService;
use feld_daemon::store::{
    ObservationUpdate, RecordFilter, RequestIdentity, ScheduleDeclaration, SchedulePatch,
    ScheduledAdmission, ScheduledAdmissionRequest, SessionWriteError, Store,
};
use serde_json::json;

mod support;

fn spawn_records(store: &Store) -> Vec<Record> {
    store
        .list_records(&RecordFilter {
            kind: Some(RecordKind::Spawned),
            ..RecordFilter::default()
        })
        .unwrap()
}

fn skip_records(
    store: &Store,
    workspace: &str,
    start: Option<&str>,
    end: Option<&str>,
) -> Vec<Record> {
    store
        .list_records(&RecordFilter {
            workspace: Some(workspace),
            actor: Some("job"),
            kind: Some(RecordKind::ScheduleSkipped),
            start,
            end,
            ..RecordFilter::default()
        })
        .unwrap()
}

fn launch() -> LaunchSpec {
    LaunchSpec {
        runtime: None,
        runtime_settings: None,
        trust_workdir: None,
        command: Some(vec!["true".to_owned()]),
        workdir: Some("/tmp".to_owned()),
        env: None,
        provider: Some("tmux".to_owned()),
        node: Some("local".to_owned()),
        provider_options: Some(json!({"socket": "feld-scheduler-test", "unit": "session"})),
    }
}

fn execution() -> Execution {
    Execution {
        runtime: None,
        execution_type: "program".to_owned(),
        label: "true program".to_owned(),
        command: Some(vec!["true".to_owned()]),
        ai: false,
        telemetry_collector_available: None,
        telemetry_runtime_prepared: None,
    }
}

fn placement() -> Placement {
    Placement {
        provider: "tmux".to_owned(),
        node: "local".to_owned(),
        workdir: "/tmp".to_owned(),
        placement: json!({
            "endpoint": {"kind": "name", "value": "feld-scheduler-test"},
            "unit": "session"
        }),
    }
}

fn schedule(store: &Store, actor: &str) -> Schedule {
    store
        .get_schedule("default", actor)
        .unwrap()
        .expect("Schedule exists")
}

#[test]
fn a_tick_past_its_time_that_left_nothing_is_overdue() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
    let store_path = temporary.path().join("feld.db");
    let store = Arc::new(Store::open(&store_path).unwrap());
    store
        .create_actor(
            "default",
            "job",
            ActorKind::Job,
            &launch(),
            Some(&ScheduleDeclaration {
                cron: "* * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
            None,
        )
        .unwrap();
    let long_ago = time_format(Utc::now() - chrono::Duration::minutes(10));
    rusqlite::Connection::open(&store_path)
        .unwrap()
        .execute(
            "UPDATE actors SET schedule_enabled_at = ?1, updated_at = ?1 WHERE name = 'job'",
            [&long_ago],
        )
        .unwrap();
    let service = |started_at| {
        ActorService::new(
            Arc::clone(&store),
            Arc::new(feld_core::plugin::Registry::load(&home, None)),
            "UTC".to_owned(),
            Arc::new(tokio::sync::Notify::new()),
            started_at,
        )
    };
    let overdue = |service: &ActorService| {
        service
            .get("default", "job")
            .unwrap()
            .unwrap()
            .schedule
            .unwrap()
            .overdue_tick
    };

    let expected = last_run_at_or_before(
        "* * * * *",
        "UTC",
        Utc::now() - chrono::Duration::seconds(60),
    )
    .unwrap()
    .unwrap();
    let running = service(Utc::now() - chrono::Duration::minutes(10));
    assert_eq!(overdue(&running), Some(expected.clone()));
    let listed = running.list("default", None, false).unwrap();
    assert_eq!(
        listed[0].schedule.as_ref().unwrap().overdue_tick,
        Some(expected.clone())
    );

    // A daemon that started after the tick never planned it.
    assert_eq!(overdue(&service(Utc::now())), None);

    // A tick admission refused fired all the same.
    store
        .admit_start(
            "default",
            "job",
            &launch(),
            &execution(),
            &placement(),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let skipped = store
        .admit_scheduled(
            "default",
            "job",
            ScheduledAdmissionRequest {
                launch: &launch(),
                schedule: &schedule(&store, "job"),
                planned: &expected,
                accepted: &time_format(Utc::now()),
                execution: &execution(),
                placement: &placement(),
            },
        )
        .unwrap();
    assert!(matches!(skipped, ScheduledAdmission::Skipped(_)));
    rusqlite::Connection::open(&store_path)
        .unwrap()
        .execute(
            "UPDATE actors SET updated_at = ?1 WHERE name = 'job'",
            [&long_ago],
        )
        .unwrap();
    assert_eq!(overdue(&running), None);

    // A tick recorded as missed is settled, not overdue.
    store
        .create_actor(
            "default",
            "missed",
            ActorKind::Job,
            &launch(),
            Some(&ScheduleDeclaration {
                cron: "* * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
            None,
        )
        .unwrap();
    rusqlite::Connection::open(&store_path)
        .unwrap()
        .execute(
            "UPDATE actors SET schedule_enabled_at = ?1, updated_at = ?1 WHERE name = 'missed'",
            [&long_ago],
        )
        .unwrap();
    let overdue_missed = || {
        running
            .get("default", "missed")
            .unwrap()
            .unwrap()
            .schedule
            .unwrap()
            .overdue_tick
    };
    assert_eq!(overdue_missed(), Some(expected.clone()));
    assert!(
        store
            .record_missed_tick("default", "missed", &expected, "it ended")
            .unwrap()
    );
    rusqlite::Connection::open(&store_path)
        .unwrap()
        .execute(
            "UPDATE actors SET updated_at = ?1 WHERE name = 'missed'",
            [&long_ago],
        )
        .unwrap();
    assert_eq!(overdue_missed(), None);
}

fn time_format(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[test]
fn scheduled_admission_is_atomic_idempotent_and_records_skip_without_spawn() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&temporary.path().join("feld.db")).unwrap());
    let launch = launch();
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
    let expected = schedule(&store, "job");
    let barrier = Arc::new(Barrier::new(3));

    let manual_store = Arc::clone(&store);
    let manual_launch = launch.clone();
    let manual_barrier = Arc::clone(&barrier);
    let manual = thread::spawn(move || {
        manual_barrier.wait();
        manual_store.admit_start(
            "default",
            "job",
            &manual_launch,
            &execution(),
            &placement(),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
    });

    let scheduled_store = Arc::clone(&store);
    let scheduled_launch = launch.clone();
    let scheduled_barrier = Arc::clone(&barrier);
    let scheduled = thread::spawn(move || {
        scheduled_barrier.wait();
        scheduled_store.admit_scheduled(
            "default",
            "job",
            ScheduledAdmissionRequest {
                launch: &scheduled_launch,
                schedule: &expected,
                planned: "2026-09-16T01:00:00Z",
                accepted: "2026-09-16T01:00:00.100Z",
                execution: &execution(),
                placement: &placement(),
            },
        )
    });

    barrier.wait();
    let manual = manual.join().unwrap();
    let scheduled = scheduled.join().unwrap().unwrap();
    let sessions = store.list_sessions(Some("default")).unwrap();
    assert_eq!(sessions.len(), 1);
    match (manual, scheduled) {
        (Ok(pending), ScheduledAdmission::Skipped(skip)) => {
            assert_eq!(pending.session.start_kind, StartKind::Start);
            assert_eq!(skip.current_session_id, pending.session.id);
            assert_eq!(spawn_records(&store).len(), 1);
        }
        (Err(SessionWriteError::Admission { .. }), ScheduledAdmission::Started(pending)) => {
            assert_eq!(pending.session.start_kind, StartKind::Schedule);
            assert_eq!(
                pending.session.planned.as_deref(),
                Some("2026-09-16T01:00:00Z")
            );
            assert!(spawn_records(&store).is_empty());
        }
        result => panic!("unexpected race result: {result:?}"),
    }

    let repeated = store
        .admit_scheduled(
            "default",
            "job",
            ScheduledAdmissionRequest {
                launch: &launch,
                schedule: &schedule(&store, "job"),
                planned: "2026-09-16T01:00:00Z",
                accepted: "2026-09-16T01:00:01Z",
                execution: &execution(),
                placement: &placement(),
            },
        )
        .unwrap();
    assert!(matches!(repeated, ScheduledAdmission::Already));
    assert_eq!(store.list_sessions(Some("default")).unwrap().len(), 1);
    assert!(skip_records(&store, "default", None, None).len() <= 1);
}

#[test]
fn scheduled_admission_closes_lost_current_and_starts_the_next_generation() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch();
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
    let first = store
        .admit_start(
            "default",
            "job",
            &launch,
            &execution(),
            &placement(),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    store
        .record_observation(
            &first.session.id,
            &ObservationUpdate {
                kind: ObservationKind::Lost,
                at: "2026-09-16T01:59:59Z".to_owned(),
                detail: None,
                exit_code: None,
            },
            None,
        )
        .unwrap();
    let mut events = store.subscribe_events();
    let admitted = store
        .admit_scheduled(
            "default",
            "job",
            ScheduledAdmissionRequest {
                launch: &launch,
                schedule: &schedule(&store, "job"),
                planned: "2026-09-16T02:00:00Z",
                accepted: "2026-09-16T02:00:00.001Z",
                execution: &execution(),
                placement: &placement(),
            },
        )
        .unwrap();
    let ScheduledAdmission::Started(second) = admitted else {
        panic!("lost current should allow the scheduled tick")
    };
    assert_eq!(second.session.start_kind, StartKind::Schedule);
    assert_eq!(second.session.generation, 2);
    assert_eq!(
        second.replaced_session.as_ref().map(|session| &session.id),
        Some(&first.session.id)
    );
    let events: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    for id in [&first.session.id, &second.session.id] {
        assert!(
            events
                .iter()
                .any(|event| event.resource == "session" && event.id == *id)
        );
    }
    assert_eq!(
        store
            .get_session(&first.session.id)
            .unwrap()
            .unwrap()
            .end_kind,
        EndKind::Unknown
    );
    assert_eq!(spawn_records(&store).len(), 1);
}

#[test]
fn skip_mark_requires_a_tick_strictly_after_the_current_session_start() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch();
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
    let current = store
        .admit_start(
            "default",
            "job",
            &launch,
            &execution(),
            &placement(),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let expected = schedule(&store, "job");
    store
        .admit_scheduled(
            "default",
            "job",
            ScheduledAdmissionRequest {
                launch: &launch,
                schedule: &expected,
                planned: "2026-09-16T03:00:00Z",
                accepted: &current.session.start,
                execution: &execution(),
                placement: &placement(),
            },
        )
        .unwrap();
    assert!(
        !store
            .get_actor("default", "job")
            .unwrap()
            .unwrap()
            .skip_mark
    );

    let after = (DateTime::parse_from_rfc3339(&current.session.start).unwrap()
        + chrono::Duration::milliseconds(1))
    .to_rfc3339();
    store
        .admit_scheduled(
            "default",
            "job",
            ScheduledAdmissionRequest {
                launch: &launch,
                schedule: &expected,
                planned: "2026-09-16T03:01:00Z",
                accepted: &after,
                execution: &execution(),
                placement: &placement(),
            },
        )
        .unwrap();
    assert!(
        store
            .get_actor("default", "job")
            .unwrap()
            .unwrap()
            .skip_mark
    );
}

#[test]
fn stale_ticks_do_nothing_and_skip_ranges_are_half_open() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch();
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
    let before_recreate = schedule(&store, "job");
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
    assert!(matches!(
        store
            .admit_scheduled(
                "default",
                "job",
                ScheduledAdmissionRequest {
                    launch: &launch,
                    schedule: &before_recreate,
                    planned: "2026-09-16T01:59:00Z",
                    accepted: "2026-09-16T01:59:00.010Z",
                    execution: &execution(),
                    placement: &placement(),
                },
            )
            .unwrap(),
        ScheduledAdmission::Stale
    ));
    let original = schedule(&store, "job");
    store
        .update_actor(
            "default",
            "job",
            None,
            Some(&SchedulePatch {
                enabled: Some(false),
                ..SchedulePatch::default()
            }),
            None,
            "UTC",
        )
        .unwrap();
    assert!(matches!(
        store
            .admit_scheduled(
                "default",
                "job",
                ScheduledAdmissionRequest {
                    launch: &launch,
                    schedule: &original,
                    planned: "2026-09-16T02:00:00Z",
                    accepted: "2026-09-16T02:00:00.010Z",
                    execution: &execution(),
                    placement: &placement(),
                },
            )
            .unwrap(),
        ScheduledAdmission::Stale
    ));
    assert!(store.list_sessions(Some("default")).unwrap().is_empty());
    assert!(skip_records(&store, "default", None, None).is_empty());

    let resumed = store
        .update_actor(
            "default",
            "job",
            None,
            Some(&SchedulePatch {
                enabled: Some(true),
                ..SchedulePatch::default()
            }),
            None,
            "UTC",
        )
        .unwrap()
        .schedule
        .expect("Schedule stays after resume");
    let current = store
        .admit_start(
            "default",
            "job",
            &launch,
            &execution(),
            &placement(),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    // The skip mark only counts ticks accepted strictly after the current Session
    // start, so derive the tick times from the Session that just started.
    let session_start = DateTime::parse_from_rfc3339(&current.session.start)
        .unwrap()
        .with_timezone(&Utc);
    let accepted = (session_start + chrono::Duration::milliseconds(10))
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let retried = (session_start + chrono::Duration::milliseconds(20))
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let after_accepted = (session_start + chrono::Duration::milliseconds(11))
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let skipped = store
        .admit_scheduled(
            "default",
            "job",
            ScheduledAdmissionRequest {
                launch: &launch,
                schedule: &resumed,
                planned: "2026-09-16T02:01:00Z",
                accepted: &accepted,
                execution: &execution(),
                placement: &placement(),
            },
        )
        .unwrap();
    assert!(matches!(skipped, ScheduledAdmission::Skipped(_)));
    assert!(matches!(
        store
            .admit_scheduled(
                "default",
                "job",
                ScheduledAdmissionRequest {
                    launch: &launch,
                    schedule: &resumed,
                    planned: "2026-09-16T02:01:00Z",
                    accepted: &retried,
                    execution: &execution(),
                    placement: &placement(),
                },
            )
            .unwrap(),
        ScheduledAdmission::Already
    ));
    assert!(skip_records(&store, "default", Some(&accepted), Some(&accepted)).is_empty());
    assert_eq!(
        skip_records(&store, "default", Some(&accepted), Some(&after_accepted)).len(),
        1
    );
    let actor = store.get_actor("default", "job").unwrap().unwrap();
    assert_eq!(
        actor
            .sessions
            .iter()
            .find(|session| session.end.is_none())
            .unwrap()
            .id,
        current.session.id
    );
    assert!(actor.skip_mark);
    assert_eq!(
        store
            .list_records(&RecordFilter {
                session: Some(&current.session.id),
                ..RecordFilter::default()
            })
            .unwrap()
            .into_iter()
            .filter(|record| record.kind == RecordKind::ScheduleSkipped)
            .count(),
        1
    );
}

struct AdvancingClock {
    now: Mutex<DateTime<Utc>>,
    sleeps: AtomicUsize,
    shutdown_after: usize,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Clock for AdvancingClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap()
    }

    async fn sleep_until(&self, deadline: DateTime<Utc>) {
        *self.now.lock().unwrap() = deadline;
        if self.sleeps.fetch_add(1, Ordering::SeqCst) >= self.shutdown_after {
            let _ = self.shutdown.send(true);
        }
    }
}

#[tokio::test]
async fn injected_clock_drives_one_tick_then_shutdown_prevents_the_next_commit() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
    home.create().unwrap();
    support::install_plugins(temporary.path());
    let store_path = home.store_path();
    let store = Arc::new(Store::open(&store_path).unwrap());
    let mut launch = launch();
    launch.provider_options = Some(json!({
        "socket": temporary.path().join("scheduler.sock").display().to_string(),
        "unit": "session"
    }));
    store.create_workspace("other").unwrap();
    for workspace in ["default", "other"] {
        store
            .create_actor(
                workspace,
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
        store
            .admit_start(
                workspace,
                "job",
                &launch,
                &execution(),
                &placement(),
                RequestIdentity {
                    requester: &Requester::Admin,
                    session_id: None,
                },
            )
            .unwrap();
    }
    store
        .create_actor("default", "impossible", ActorKind::Job, &launch, None, None)
        .unwrap();
    rusqlite::Connection::open(store_path)
        .unwrap()
        .execute(
            "UPDATE actors SET schedule_cron = '0 0 31 2 *', schedule_timezone = 'UTC',
             schedule_enabled = 1, schedule_created_at = '2026-09-16T00:00:00.000Z',
             schedule_enabled_at = '2026-09-16T00:00:00.000Z', schedule_revision = 1
             WHERE workspace_key = 'default' AND name = 'impossible'",
            [],
        )
        .unwrap();

    let controller = Arc::new(SessionController::new(
        Arc::clone(&store),
        home,
        "http://127.0.0.1:1".to_owned(),
    ));
    let scheduler = Scheduler::new(
        Arc::clone(&store),
        controller,
        Arc::new(tokio::sync::Notify::new()),
    );
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let clock = AdvancingClock {
        now: Mutex::new("2026-09-16T00:00:30Z".parse().unwrap()),
        sleeps: AtomicUsize::new(0),
        shutdown_after: 1,
        shutdown: shutdown_tx,
    };
    scheduler.run_with_clock(&clock, shutdown_rx).await.unwrap();

    for workspace in ["default", "other"] {
        let skips = skip_records(&store, workspace, None, None);
        assert_eq!(skips.len(), 1);
        assert_eq!(skips[0].data["planned"], "2026-09-16T00:01:00.000Z");
    }
    assert!(
        store
            .get_actor("default", "impossible")
            .unwrap()
            .unwrap()
            .sessions
            .is_empty()
    );
}

/// A clock that wakes the Scheduler once, at the first deadline, and never again; each
/// reading after the wake-up is a millisecond later.
struct WakeOnceClock {
    now: Mutex<DateTime<Utc>>,
    sleeps: AtomicUsize,
}

impl Clock for WakeOnceClock {
    fn now(&self) -> DateTime<Utc> {
        let mut now = self.now.lock().unwrap();
        let read = *now;
        *now += chrono::Duration::milliseconds(1);
        read
    }

    async fn sleep_until(&self, deadline: DateTime<Utc>) {
        if self.sleeps.fetch_add(1, Ordering::SeqCst) == 0 {
            *self.now.lock().unwrap() = deadline;
        } else {
            std::future::pending::<()>().await;
        }
    }
}

/// A wall clock that moves only when the test sets it; the Scheduler sleeping on it reads
/// it again every 100 ms.
#[derive(Clone)]
struct WallClock(Arc<Mutex<DateTime<Utc>>>);

impl WallClock {
    fn at(now: &str) -> Self {
        Self(Arc::new(Mutex::new(now.parse().unwrap())))
    }

    fn set(&self, now: &str) {
        *self.0.lock().unwrap() = now.parse().unwrap();
    }
}

impl Clock for WallClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }

    async fn sleep_until(&self, deadline: DateTime<Utc>) {
        while self.now() < deadline {
            tokio::time::sleep(StdDuration::from_millis(100)).await;
        }
    }
}

fn supervised_fixture() -> (tempfile::TempDir, Arc<Store>, Scheduler) {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
    home.create().unwrap();
    let store = Arc::new(Store::open(&home.store_path()).unwrap());
    store
        .create_actor(
            "default",
            "job",
            ActorKind::Job,
            &launch(),
            Some(&ScheduleDeclaration {
                cron: "* * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
            None,
        )
        .unwrap();
    // The Schedule was enabled and last changed before the ticks the tests play.
    rusqlite::Connection::open(home.store_path())
        .unwrap()
        .execute(
            "UPDATE actors SET schedule_enabled_at = '2026-09-15T00:00:00.000Z',
               updated_at = '2026-09-15T00:00:00.000Z' WHERE name = 'job'",
            [],
        )
        .unwrap();
    let controller = Arc::new(SessionController::new(
        Arc::clone(&store),
        home,
        "http://127.0.0.1:1".to_owned(),
    ));
    let scheduler = Scheduler::new(
        Arc::clone(&store),
        controller,
        Arc::new(tokio::sync::Notify::new()),
    );
    (temporary, store, scheduler)
}

const QUICK_WATCH: Watch = Watch {
    interval: StdDuration::from_millis(20),
    stall_after: chrono::Duration::zero(),
    restart_after: StdDuration::from_millis(20),
    restart_max: StdDuration::from_millis(20),
    healthy_after: StdDuration::from_secs(60),
};

async fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + StdDuration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what} did not happen");
        tokio::time::sleep(StdDuration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn a_scheduler_that_stops_waking_is_started_again_and_its_ticks_recorded_as_missed() {
    let (_temporary, store, scheduler) = supervised_fixture();
    let status = scheduler.status();
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&runs);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    // The supervisor looks when the stalled Scheduler has let three ticks go by; its wall
    // clock creeps a millisecond a reading.
    let supervisor = tokio::spawn(scheduler.supervise_with(
        shutdown_rx,
        QUICK_WATCH,
        WakeOnceClock {
            now: Mutex::new("2026-09-16T00:03:30Z".parse().unwrap()),
            sleeps: AtomicUsize::new(0),
        },
        move |scheduler, shutdown| {
            let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
            async move {
                if !first {
                    return std::future::pending().await;
                }
                let clock = WakeOnceClock {
                    now: Mutex::new("2026-09-16T00:00:30Z".parse().unwrap()),
                    sleeps: AtomicUsize::new(0),
                };
                let _ = scheduler.run_with_clock(&clock, shutdown).await;
            }
        },
    ));

    wait_for("a restart", || status.lock().unwrap().restarts >= 1).await;
    {
        let status = status.lock().unwrap();
        // The Scheduler woke for the tick, and the launch it failed left it asleep.
        assert_eq!(status.woke_at.as_deref(), Some("2026-09-16T00:01:00.000Z"));
        let restart = status.last_restart.as_ref().unwrap();
        assert_eq!(restart.reason, "it has not woken for its tick");
        assert!(
            restart.at.starts_with("2026-09-16T00:03:30."),
            "{}",
            restart.at
        );
    }
    let missed = store
        .list_records(&RecordFilter {
            kind: Some(RecordKind::ScheduleMissed),
            ..RecordFilter::default()
        })
        .unwrap();
    let planned: Vec<&str> = missed
        .iter()
        .map(|record| record.data["planned"].as_str().unwrap())
        .collect();
    assert_eq!(
        planned,
        [
            "2026-09-16T00:01:00.000Z",
            "2026-09-16T00:02:00.000Z",
            "2026-09-16T00:03:00.000Z"
        ]
    );
    assert!(missed.iter().all(|record| record.session_id.is_none()));
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    shutdown_tx.send(true).unwrap();
    tokio::time::timeout(StdDuration::from_secs(5), supervisor)
        .await
        .expect("shutdown ended the supervisor")
        .unwrap();
}

#[tokio::test]
async fn a_scheduler_that_fails_or_ends_is_started_again_and_shutdown_ends_a_stuck_one() {
    let (_temporary, _store, scheduler) = supervised_fixture();
    let status = scheduler.status();
    let runs = Arc::new(AtomicUsize::new(0));
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let counted = Arc::clone(&runs);
    let supervisor = tokio::spawn(scheduler.supervise_with(
        shutdown_rx,
        QUICK_WATCH,
        SystemClock,
        move |_, _| {
            let run = counted.fetch_add(1, Ordering::SeqCst);
            async move {
                match run {
                    0 => panic!("injected fault"),
                    1 => {}
                    // Stuck where it never looks at shutdown.
                    _ => std::future::pending().await,
                }
            }
        },
    ));

    wait_for("two restarts", || runs.load(Ordering::SeqCst) >= 3).await;
    let status = status.lock().unwrap().clone();
    assert_eq!(status.restarts, 2);
    assert_eq!(status.last_restart.unwrap().reason, "it ended");

    shutdown_tx.send(true).unwrap();
    tokio::time::timeout(StdDuration::from_secs(5), supervisor)
        .await
        .expect("shutdown waited on a stuck Scheduler")
        .unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn a_scheduler_that_keeps_ending_is_started_again_after_a_growing_wait() {
    let (_temporary, _store, scheduler) = supervised_fixture();
    let starts = Arc::new(Mutex::new(Vec::new()));
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let started = Arc::clone(&starts);
    let supervisor = tokio::spawn(scheduler.supervise_with(
        shutdown_rx,
        Watch::default(),
        SystemClock,
        move |_, _| {
            let run = {
                let mut starts = started.lock().unwrap();
                starts.push(tokio::time::Instant::now());
                starts.len() - 1
            };
            async move {
                match run {
                    // Runs for a minute before it ends.
                    8 => tokio::time::sleep(StdDuration::from_secs(61)).await,
                    9.. => std::future::pending().await,
                    _ => {}
                }
            }
        },
    ));

    wait_for("ten runs", || starts.lock().unwrap().len() >= 10).await;
    let gaps: Vec<u64> = starts
        .lock()
        .unwrap()
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).as_secs())
        .collect();
    // The wait doubles up to a minute, and starts over after a Scheduler that ran a minute.
    assert_eq!(gaps, [1, 2, 4, 8, 16, 32, 60, 60, 61 + 1]);

    shutdown_tx.send(true).unwrap();
    tokio::time::timeout(StdDuration::from_secs(5), supervisor)
        .await
        .expect("shutdown ended the supervisor")
        .unwrap();
}

#[tokio::test]
async fn a_wall_clock_jump_past_the_tick_gives_the_scheduler_time_to_wake() {
    let (_temporary, store, scheduler) = supervised_fixture();
    let status = scheduler.status();
    let clock = WallClock::at("2026-09-16T00:00:30Z");
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let run_clock = clock.clone();
    let supervisor = tokio::spawn(scheduler.supervise_with(
        shutdown_rx,
        Watch {
            stall_after: chrono::Duration::minutes(1),
            ..QUICK_WATCH
        },
        clock.clone(),
        move |scheduler, shutdown| {
            let clock = run_clock.clone();
            async move {
                let _ = scheduler.run_with_clock(&clock, shutdown).await;
            }
        },
    ));
    wait_for("the Scheduler to wait for its tick", || {
        !status.lock().unwrap().planned.is_empty()
    })
    .await;

    // The host sleeps across the tick and wakes ten minutes later.
    clock.set("2026-09-16T00:11:00Z");
    wait_for("the Scheduler to wake", || {
        status.lock().unwrap().woke_at.as_deref() == Some("2026-09-16T00:11:00.000Z")
    })
    .await;
    tokio::time::sleep(StdDuration::from_millis(200)).await;
    assert_eq!(status.lock().unwrap().restarts, 0);
    // The Scheduler itself, not a restart, records the ticks the host slept through.
    let missed = store
        .list_records(&RecordFilter {
            kind: Some(RecordKind::ScheduleMissed),
            ..RecordFilter::default()
        })
        .unwrap();
    assert_eq!(missed.len(), 11);
    assert_eq!(missed[0].data["planned"], "2026-09-16T00:01:00.000Z");
    assert_eq!(missed[10].data["planned"], "2026-09-16T00:11:00.000Z");
    assert!(
        missed
            .iter()
            .all(|record| record.data["reason"]
                == "the Scheduler reached it more than 2 minutes late")
    );

    shutdown_tx.send(true).unwrap();
    tokio::time::timeout(StdDuration::from_secs(5), supervisor)
        .await
        .expect("shutdown ended the supervisor")
        .unwrap();
}

struct DaemonFixture {
    temporary: tempfile::TempDir,
}

impl DaemonFixture {
    fn command(&self, arguments: &[&str]) -> Output {
        // The fixture owns HOME too, so nothing this runs reads or writes the real one.
        support::feld()
            .arg("--home")
            .arg(self.temporary.path())
            .env("HOME", self.temporary.path())
            .env_remove("TMUX")
            .args(arguments)
            .output()
            .unwrap()
    }

    fn json(&self, arguments: &[&str]) -> serde_json::Value {
        let output = self.command(arguments);
        assert!(
            output.status.success(),
            "feld {} failed:\nstdout: {}\nstderr: {}\nlog: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(self.temporary.path().join("daemon.log")).unwrap_or_default()
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for DaemonFixture {
    fn drop(&mut self) {
        let _ = self.command(&["daemon", "stop"]);
        let _ = Command::new("tmux")
            .arg("-S")
            .arg(self.temporary.path().join("cron.sock"))
            .arg("kill-server")
            .output();
    }
}

#[test]
fn real_daemon_cron_tick_launches_a_tmux_program_without_a_spawn_record() {
    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = DaemonFixture {
        temporary: tempfile::tempdir().unwrap(),
    };
    support::install_plugins(fixture.temporary.path());
    fixture.json(&["daemon", "start", "--json"]);
    let impossible = fixture.command(&[
        "actor",
        "create",
        "impossible",
        "--kind",
        "job",
        "--cron",
        "0 0 31 2 *",
        "--timezone",
        "UTC",
        "--json",
        "--",
        "true",
    ]);
    assert_eq!(impossible.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&impossible.stderr)
            .contains("no occurrence within the next 400 years")
    );
    let socket = fixture.temporary.path().join("cron.sock");
    let marker = fixture.temporary.path().join("tick.txt");
    let script = format!("printf tick > '{}'", marker.display());
    let missing_workdir = fixture.temporary.path().join("removed-before-tick");
    fs::create_dir(&missing_workdir).unwrap();
    fixture.json(&[
        "actor",
        "create",
        "cron-bad-workdir",
        "--kind",
        "job",
        "--cron",
        "* * * * *",
        "--timezone",
        "UTC",
        "--workdir",
        missing_workdir.to_str().unwrap(),
        "--tmux-socket",
        socket.to_str().unwrap(),
        "--json",
        "--",
        "true",
    ]);
    fs::remove_dir(&missing_workdir).unwrap();
    fixture.json(&[
        "actor",
        "create",
        "cron-e2e",
        "--kind",
        "job",
        "--cron",
        "* * * * *",
        "--timezone",
        "UTC",
        "--tmux-socket",
        socket.to_str().unwrap(),
        "--json",
        "--",
        "sh",
        "-c",
        &script,
    ]);

    let seconds_to_boundary = 61 - u64::from(Utc::now().second());
    thread::sleep(StdDuration::from_secs(seconds_to_boundary));
    let deadline = Instant::now() + StdDuration::from_secs(10);
    let described = loop {
        let described = fixture.json(&["actor", "get", "cron-e2e", "--json"]);
        if described["actor"]["latestSession"]["startKind"] == "schedule" && marker.is_file() {
            break described;
        }
        assert!(
            Instant::now() < deadline,
            "cron tick did not launch before the deadline: {described}"
        );
        thread::sleep(StdDuration::from_millis(100));
    };
    assert!(described["actor"]["latestSession"]["planned"].is_string());
    let bad = loop {
        let described = fixture.json(&["actor", "get", "cron-bad-workdir", "--json"]);
        if described["actor"]["latestSession"]["end"].is_string() {
            break described;
        }
        assert!(
            Instant::now() < deadline,
            "failed preparation did not close its planned Session: {described}"
        );
        thread::sleep(StdDuration::from_millis(25));
    };
    assert_eq!(bad["actor"]["latestSession"]["startKind"], "schedule");
    assert_eq!(bad["actor"]["latestSession"]["observation"]["kind"], "gone");
    assert!(described["actor"]["schedule"]["overdueTick"].is_null());
    let status = fixture.json(&["daemon", "status", "--json"]);
    let scheduler = &status["scheduler"];
    assert!(scheduler["wokeAt"].is_string(), "{status}");
    assert_eq!(scheduler["restarts"], 0);
    let planned = scheduler["planned"].as_array().unwrap();
    assert!(
        planned.iter().any(|tick| tick["actor"] == "cron-e2e"
            && tick["planned"].as_str().unwrap()
                > described["actor"]["latestSession"]["planned"]
                    .as_str()
                    .unwrap()),
        "{status}"
    );
    let token = fs::read_to_string(fixture.temporary.path().join("admin/api-token")).unwrap();
    let spawns: serde_json::Value = reqwest::blocking::Client::new()
        .get(format!("{}/v1/records", status["api"].as_str().unwrap()))
        .query(&[("kind", "spawned")])
        .bearer_auth(token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(spawns["records"].as_array().unwrap().is_empty());
}

/// A rename plans the Schedule again, so the next tick starts the Actor by its new name.
#[test]
fn a_renamed_job_ticks_by_its_new_name() {
    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = DaemonFixture {
        temporary: tempfile::tempdir().unwrap(),
    };
    support::install_plugins(fixture.temporary.path());
    fixture.json(&["daemon", "start", "--json"]);
    let socket = fixture.temporary.path().join("rename.sock");
    let created = fixture.json(&[
        "actor",
        "create",
        "cron-before",
        "--kind",
        "job",
        "--cron",
        "* * * * *",
        "--timezone",
        "UTC",
        "--tmux-socket",
        socket.to_str().unwrap(),
        "--json",
        "--",
        "true",
    ]);
    let planned_actors = || -> Vec<String> {
        fixture.json(&["daemon", "status", "--json"])["scheduler"]["planned"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tick| tick["actor"].as_str().unwrap().to_owned())
            .collect()
    };
    let deadline = Instant::now() + StdDuration::from_secs(10);
    while !planned_actors().contains(&"cron-before".to_owned()) {
        assert!(Instant::now() < deadline, "the Schedule was not planned");
        thread::sleep(StdDuration::from_millis(50));
    }

    fixture.json(&[
        "actor",
        "update",
        "cron-before",
        "--name",
        "cron-after",
        "--json",
    ]);
    let deadline = Instant::now() + StdDuration::from_secs(10);
    loop {
        let planned = planned_actors();
        if planned.contains(&"cron-after".to_owned())
            && !planned.contains(&"cron-before".to_owned())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the rename was not planned: {planned:?}"
        );
        thread::sleep(StdDuration::from_millis(50));
    }

    let seconds_to_boundary = 61 - u64::from(Utc::now().second());
    thread::sleep(StdDuration::from_secs(seconds_to_boundary));
    let deadline = Instant::now() + StdDuration::from_secs(10);
    loop {
        let described = fixture.json(&["actor", "get", "cron-after", "--json"]);
        if described["actor"]["latestSession"]["startKind"] == "schedule" {
            assert_eq!(described["actor"]["id"], created["actor"]["id"]);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the renamed Job did not tick: {described}"
        );
        thread::sleep(StdDuration::from_millis(100));
    }
}

#[tokio::test]
async fn ticks_of_a_cron_changed_while_the_scheduler_stalls_are_recorded_as_missed() {
    let (_temporary, store, scheduler) = supervised_fixture();
    let minute = |at: DateTime<Utc>| at.with_second(0).unwrap().with_nanosecond(0).unwrap();
    let start = minute(Utc::now()) - chrono::Duration::minutes(3);
    let restart_at = start + chrono::Duration::seconds(330);
    let status = scheduler.status();
    let wall = WallClock(Arc::new(Mutex::new(
        restart_at - chrono::Duration::milliseconds(10),
    )));
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&runs);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let supervisor = tokio::spawn(scheduler.supervise_with(
        shutdown_rx,
        QUICK_WATCH,
        wall.clone(),
        move |scheduler, shutdown| {
            let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
            async move {
                if !first {
                    return std::future::pending().await;
                }
                let clock = WakeOnceClock {
                    now: Mutex::new(start + chrono::Duration::seconds(30)),
                    sleeps: AtomicUsize::new(0),
                };
                let _ = scheduler.run_with_clock(&clock, shutdown).await;
            }
        },
    ));

    let woke = time_format(start + chrono::Duration::minutes(1));
    wait_for("the Scheduler to wake for its tick", || {
        status.lock().unwrap().woke_at.as_deref() == Some(woke.as_str())
    })
    .await;
    // The new cron leaves out the tick the stalled Scheduler waits for.
    let cron = format!(
        "{} * * * *",
        (3..6)
            .map(|offset| (start + chrono::Duration::minutes(offset))
                .minute()
                .to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let updated_at = store
        .update_actor(
            "default",
            "job",
            None,
            Some(&SchedulePatch {
                cron: Some(cron.clone()),
                ..SchedulePatch::default()
            }),
            None,
            "UTC",
        )
        .unwrap()
        .updated_at;
    wall.set(&time_format(restart_at));
    wait_for("a restart", || status.lock().unwrap().restarts >= 1).await;

    // The ticks of the new cron after the change up to the restart, and none from before it.
    let mut expected = Vec::new();
    let mut after = updated_at;
    loop {
        after = next_run_after(&cron, "UTC", after.parse().unwrap()).unwrap();
        if after.parse::<DateTime<Utc>>().unwrap() > restart_at {
            break;
        }
        expected.push(after.clone());
    }
    assert!(!expected.is_empty());
    let missed: Vec<String> = store
        .list_records(&RecordFilter {
            kind: Some(RecordKind::ScheduleMissed),
            ..RecordFilter::default()
        })
        .unwrap()
        .iter()
        .map(|record| record.data["planned"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(missed, expected);

    shutdown_tx.send(true).unwrap();
    tokio::time::timeout(StdDuration::from_secs(5), supervisor)
        .await
        .expect("shutdown ended the supervisor")
        .unwrap();
}

#[tokio::test]
async fn ticks_of_a_cron_changed_before_the_tick_the_scheduler_waits_for_are_recorded_as_missed() {
    let (temporary, store, scheduler) = supervised_fixture();
    let status = scheduler.status();
    // The supervisor's wall clock stands still until the change is done, then creeps
    // forward without a jump.
    let wall = WallClock::at("2026-09-16T00:03:29.990Z");
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&runs);
    // The Scheduler wakes at 00:00:30 and never again, waiting for 00:01.
    let supervisor = tokio::spawn(scheduler.supervise_with(
        shutdown_rx,
        QUICK_WATCH,
        wall.clone(),
        move |scheduler, shutdown| {
            let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
            async move {
                if !first {
                    return std::future::pending().await;
                }
                let clock = WallClock::at("2026-09-16T00:00:30Z");
                let _ = scheduler.run_with_clock(&clock, shutdown).await;
            }
        },
    ));

    wait_for("the Scheduler to wait for its tick", || {
        let status = status.lock().unwrap();
        status.planned.first().map(|tick| tick.planned.as_str()) == Some("2026-09-16T00:01:00.000Z")
    })
    .await;
    // The cron changes after the Scheduler last woke and before the tick it waits for,
    // which the new cron leaves out.
    store
        .update_actor(
            "default",
            "job",
            None,
            Some(&SchedulePatch {
                cron: Some("2,3 * * * *".to_owned()),
                ..SchedulePatch::default()
            }),
            None,
            "UTC",
        )
        .unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
    rusqlite::Connection::open(home.store_path())
        .unwrap()
        .execute(
            "UPDATE actors SET updated_at = '2026-09-16T00:00:45.000Z' WHERE name = 'job'",
            [],
        )
        .unwrap();
    wall.set("2026-09-16T00:03:30Z");
    wait_for("a restart", || status.lock().unwrap().restarts >= 1).await;

    let missed: Vec<String> = store
        .list_records(&RecordFilter {
            kind: Some(RecordKind::ScheduleMissed),
            ..RecordFilter::default()
        })
        .unwrap()
        .iter()
        .map(|record| record.data["planned"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        missed,
        ["2026-09-16T00:02:00.000Z", "2026-09-16T00:03:00.000Z"]
    );

    shutdown_tx.send(true).unwrap();
    tokio::time::timeout(StdDuration::from_secs(5), supervisor)
        .await
        .expect("shutdown ended the supervisor")
        .unwrap();
}

#[tokio::test]
async fn ticks_of_schedules_resumed_while_the_scheduler_stalls_are_recorded_as_missed() {
    let (_temporary, store, scheduler) = supervised_fixture();
    let set_enabled = |actor: &str, enabled: bool| {
        store
            .update_actor(
                "default",
                actor,
                None,
                Some(&SchedulePatch {
                    enabled: Some(enabled),
                    ..SchedulePatch::default()
                }),
                None,
                "UTC",
            )
            .unwrap();
    };
    // `paused` is paused before the Scheduler plans; `job` is planned, then paused and
    // resumed while the Scheduler stalls.
    store
        .create_actor(
            "default",
            "paused",
            ActorKind::Job,
            &launch(),
            Some(&ScheduleDeclaration {
                cron: "* * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
            None,
        )
        .unwrap();
    set_enabled("paused", false);
    let minute = |at: DateTime<Utc>| at.with_second(0).unwrap().with_nanosecond(0).unwrap();
    let start = minute(Utc::now()) - chrono::Duration::minutes(3);
    // The supervisor's wall clock stands still until the resumes are done, then creeps
    // forward without a jump, so the supervisor sees the stall only then.
    let restart_at = start + chrono::Duration::seconds(330);
    let status = scheduler.status();
    let wall = WallClock(Arc::new(Mutex::new(
        restart_at - chrono::Duration::milliseconds(10),
    )));
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&runs);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let supervisor = tokio::spawn(scheduler.supervise_with(
        shutdown_rx,
        QUICK_WATCH,
        wall.clone(),
        move |scheduler, shutdown| {
            let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
            async move {
                if !first {
                    return std::future::pending().await;
                }
                let clock = WakeOnceClock {
                    now: Mutex::new(start + chrono::Duration::seconds(30)),
                    sleeps: AtomicUsize::new(0),
                };
                let _ = scheduler.run_with_clock(&clock, shutdown).await;
            }
        },
    ));

    let woke = time_format(start + chrono::Duration::minutes(1));
    wait_for("the Scheduler to wake for its tick", || {
        status.lock().unwrap().woke_at.as_deref() == Some(woke.as_str())
    })
    .await;
    set_enabled("job", false);
    set_enabled("job", true);
    set_enabled("paused", true);
    wall.set(&time_format(restart_at));
    wait_for("a restart", || status.lock().unwrap().restarts >= 1).await;

    for actor in ["paused", "job"] {
        // The ticks after the resume up to the restart, and none from before it.
        let enabled_at = store
            .get_schedule("default", actor)
            .unwrap()
            .unwrap()
            .enabled_at
            .unwrap();
        let mut expected = Vec::new();
        let mut after = enabled_at;
        loop {
            after = next_run_after("* * * * *", "UTC", after.parse().unwrap()).unwrap();
            if after.parse::<DateTime<Utc>>().unwrap() > restart_at {
                break;
            }
            expected.push(after.clone());
        }
        assert!(!expected.is_empty(), "{actor}");
        let missed: Vec<String> = store
            .list_records(&RecordFilter {
                actor: Some(actor),
                kind: Some(RecordKind::ScheduleMissed),
                ..RecordFilter::default()
            })
            .unwrap()
            .iter()
            .map(|record| record.data["planned"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(missed, expected, "{actor}");
    }

    shutdown_tx.send(true).unwrap();
    tokio::time::timeout(StdDuration::from_secs(5), supervisor)
        .await
        .expect("shutdown ended the supervisor")
        .unwrap();
}

#[tokio::test]
async fn ticks_the_host_slept_through_are_recorded_as_missed_and_the_next_one_fires() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
    home.create().unwrap();
    support::install_plugins(temporary.path());
    let store_path = home.store_path();
    let store = Arc::new(Store::open(&store_path).unwrap());
    let mut launch = launch();
    launch.provider_options = Some(json!({
        "socket": temporary.path().join("scheduler.sock").display().to_string(),
        "unit": "session"
    }));
    store
        .create_actor(
            "default",
            "job",
            ActorKind::Job,
            &launch,
            Some(&ScheduleDeclaration {
                cron: "*/5 * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
            None,
        )
        .unwrap();
    // A running Session makes each tick that fires leave a `schedule.skipped`.
    store
        .admit_start(
            "default",
            "job",
            &launch,
            &execution(),
            &placement(),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let controller = Arc::new(SessionController::new(
        Arc::clone(&store),
        home,
        "http://127.0.0.1:1".to_owned(),
    ));
    let scheduler = Scheduler::new(
        Arc::clone(&store),
        controller,
        Arc::new(tokio::sync::Notify::new()),
    );
    let status = scheduler.status();
    let clock = WallClock::at("2026-09-16T00:00:30Z");
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let run_clock = clock.clone();
    let run = tokio::spawn(async move { scheduler.run_with_clock(&run_clock, shutdown_rx).await });
    let planned = || {
        status
            .lock()
            .unwrap()
            .planned
            .iter()
            .map(|tick| tick.planned.clone())
            .collect::<Vec<_>>()
    };
    wait_for("the Scheduler to wait for its tick", || {
        planned() == ["2026-09-16T00:05:00.000Z"]
    })
    .await;

    // The host sleeps for eight hours, and the first attempt to record the ticks it slept
    // through fails.
    let store_db = rusqlite::Connection::open(&store_path).unwrap();
    store_db
        .execute_batch(
            "CREATE TRIGGER refuse_missed BEFORE INSERT ON records
             WHEN NEW.kind = 'schedule.missed'
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        )
        .unwrap();
    clock.set("2026-09-16T08:00:30Z");
    tokio::time::sleep(StdDuration::from_millis(500)).await;
    assert_eq!(planned(), ["2026-09-16T00:05:00.000Z"]);
    store_db
        .execute_batch("DROP TRIGGER refuse_missed")
        .unwrap();
    // The Scheduler tries again a second later.
    clock.set("2026-09-16T08:00:31Z");
    wait_for("the Scheduler to plan from the time it woke", || {
        planned() == ["2026-09-16T08:05:00.000Z"]
    })
    .await;
    assert!(skip_records(&store, "default", None, None).is_empty());
    assert_eq!(
        store
            .get_actor("default", "job")
            .unwrap()
            .unwrap()
            .sessions
            .len(),
        1
    );
    let missed = store
        .list_records(&RecordFilter {
            kind: Some(RecordKind::ScheduleMissed),
            ..RecordFilter::default()
        })
        .unwrap();
    assert_eq!(missed.len(), 96);
    assert_eq!(missed[0].data["planned"], "2026-09-16T00:05:00.000Z");
    assert_eq!(missed[95].data["planned"], "2026-09-16T08:00:00.000Z");
    assert!(
        missed
            .iter()
            .all(|record| record.data["reason"]
                == "the Scheduler reached it more than 2 minutes late")
    );

    // The next tick fires, even forty seconds late.
    clock.set("2026-09-16T08:05:40Z");
    wait_for("the next tick to fire", || {
        !skip_records(&store, "default", None, None).is_empty()
    })
    .await;
    let skips = skip_records(&store, "default", None, None);
    assert_eq!(skips.len(), 1);
    assert_eq!(skips[0].data["planned"], "2026-09-16T08:05:00.000Z");

    shutdown_tx.send(true).unwrap();
    run.await.unwrap().unwrap();
}

#[test]
fn ticks_of_a_schedule_changed_since_they_were_planned_are_not_recorded_as_missed() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    store
        .create_actor(
            "default",
            "job",
            ActorKind::Job,
            &launch(),
            Some(&ScheduleDeclaration {
                cron: "*/5 * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
            None,
        )
        .unwrap();
    let planned = schedule(&store, "job");
    store
        .update_actor(
            "default",
            "job",
            None,
            Some(&SchedulePatch {
                cron: Some("*/10 * * * *".to_owned()),
                ..SchedulePatch::default()
            }),
            None,
            "UTC",
        )
        .unwrap();
    let ticks = [
        "2026-09-16T00:05:00.000Z".to_owned(),
        "2026-09-16T00:10:00.000Z".to_owned(),
    ];
    let missed = || {
        store
            .list_records(&RecordFilter {
                kind: Some(RecordKind::ScheduleMissed),
                ..RecordFilter::default()
            })
            .unwrap()
    };
    assert!(
        !store
            .record_missed_ticks_of("default", "job", &planned, &ticks, "late")
            .unwrap()
    );
    assert!(missed().is_empty());

    let current = schedule(&store, "job");
    assert!(
        store
            .record_missed_ticks_of("default", "job", &current, &ticks, "late")
            .unwrap()
    );
    assert_eq!(missed().len(), 2);
}
