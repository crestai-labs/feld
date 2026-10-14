use serde_json::{Value, json};

use super::*;

const AT: &str = "2026-09-26T00:00:00.000Z";
const LATER: &str = "2026-09-26T01:00:00.000Z";

fn observation(kind: ObservationKind) -> Observation {
    Observation {
        kind,
        at: AT.to_owned(),
        since: None,
        detail: None,
        exit_code: None,
    }
}

fn action(pending: bool) -> LifecycleAction {
    LifecycleAction {
        accepted: AT.to_owned(),
        requester_session_id: None,
        attempt: (!pending).then(|| "stop".to_owned()),
        observed: None,
    }
}

fn lifecycle(stop: Option<bool>, kill: Option<bool>) -> SessionLifecycle {
    SessionLifecycle {
        stop: stop.map(action),
        kill: kill.map(action),
        ..SessionLifecycle::default()
    }
}

fn schedule(enabled: bool) -> Schedule {
    Schedule::new("0 * * * *".to_owned(), "UTC".to_owned(), enabled, AT)
}

fn session(start: &str, end: Option<&str>) -> Session {
    Session {
        id: "session_1".to_owned(),
        workspace: "default".to_owned(),
        actor_id: "actor_1".to_owned(),
        actor: "job".to_owned(),
        actor_kind: ActorKind::Job,
        generation: 1,
        start: start.to_owned(),
        end: end.map(str::to_owned),
        start_kind: StartKind::Start,
        ownership: Ownership::Owned,
        end_kind: if end.is_some() {
            EndKind::Exited
        } else {
            EndKind::Running
        },
        exit_code: end.map(|_| 0),
        planned: None,
        execution: Execution {
            execution_type: "program".to_owned(),
            runtime: None,
            label: "program".to_owned(),
            command: Some(vec!["true".to_owned()]),
            ai: false,
            telemetry_collector_available: None,
            telemetry_runtime_prepared: None,
        },
        placement: Placement {
            provider: "tmux".to_owned(),
            node: "local".to_owned(),
            workdir: "/".to_owned(),
            placement: json!({}),
        },
        lifecycle: SessionLifecycle::default(),
        observation: None,
        model: None,
        spawned_by: None,
        turn: None,
        last_entry_at: None,
        outcome: Outcome::Running,
    }
}

fn actor(sessions: Vec<Session>, waiting_count: u64) -> Actor {
    Actor {
        id: "actor_1".to_owned(),
        name: "job".to_owned(),
        workspace: "default".to_owned(),
        kind: ActorKind::Job,
        launch_spec: None,
        schedule: None,
        grants: Default::default(),
        accept_from: None,
        created_at: AT.to_owned(),
        updated_at: AT.to_owned(),
        deleted_at: None,
        current_session: None,
        latest_session: None,
        sessions,
        attention: false,
        waiting_count,
        unread_mail: false,
        skip_mark: false,
        unlinked_mark: false,
    }
}

#[test]
fn a_session_with_a_turn_keeps_a_null_last_entry_at() {
    let ended = serde_json::to_value(session(AT, Some(AT))).unwrap();
    assert!(ended.get("turn").is_none() && ended.get("lastEntryAt").is_none());
    let mut current = session(AT, None);
    current.turn = Some(Turn::Unavailable);
    current.last_entry_at = Some(None);
    let written = serde_json::to_value(&current).unwrap();
    assert_eq!(written["turn"], "unavailable");
    assert_eq!(written.get("lastEntryAt"), Some(&Value::Null));
    // What the CLI reads back from the daemon it writes again unchanged.
    let read: Session = serde_json::from_value(written.clone()).unwrap();
    assert_eq!(read, current);
    assert_eq!(serde_json::to_value(&read).unwrap(), written);
}

#[test]
fn admission_closes_what_cannot_run_and_refuses_the_rest() {
    use ObservationKind::*;
    assert_eq!(admission(Some(Lost)), Admission::CloseUnknown);
    assert_eq!(admission(Some(Mismatched)), Admission::CloseUnknown);
    assert_eq!(admission(Some(Gone)), Admission::CloseExited);
    assert!(Running.settles_start() && !Relocated.settles_start());
    assert!(Gone.ends_execution() && !Lost.ends_execution());
    for observation in [None, Some(Running), Some(Relocated), Some(Unknown)] {
        assert_eq!(admission(observation), Admission::Refuse, "{observation:?}");
    }
}

#[test]
fn a_gone_session_ends_stopped_only_when_requested() {
    assert_eq!(end_kind_for_gone(true), EndKind::Stopped);
    assert_eq!(end_kind_for_gone(false), EndKind::Exited);
    assert!(lifecycle(Some(false), None).requested());
    assert!(lifecycle(None, Some(true)).requested());
    assert!(!lifecycle(None, None).requested());
}

#[test]
fn lifecycle_accepts_a_new_action_and_resumes_a_pending_one() {
    use LifecycleAcceptance::*;
    use LifecycleActionKind::*;
    let cases = [
        (None, None, Stop, Accept),
        (None, None, Kill, Accept),
        (Some(true), None, Stop, Resume),
        (Some(false), None, Stop, Already),
        (Some(true), None, Kill, Accept),
        (None, Some(true), Kill, Resume),
        (None, Some(false), Kill, Already),
        // A kill covers a stop, pending or not.
        (None, Some(true), Stop, Already),
        (Some(true), Some(false), Stop, Already),
    ];
    for (stop, kill, kind, expected) in cases {
        assert_eq!(
            lifecycle(stop, kill).accept(kind),
            expected,
            "stop {stop:?} kill {kill:?} {kind:?}"
        );
    }
}

#[test]
fn lifecycle_resumes_an_action_whose_outcome_is_unknown() {
    use LifecycleActionKind::*;
    for (attempt, kind, expected) in [
        (None, ObservationKind::Unknown, LifecycleAcceptance::Resume),
        (
            Some("stop"),
            ObservationKind::Unknown,
            LifecycleAcceptance::Resume,
        ),
        (
            Some("stop"),
            ObservationKind::Running,
            LifecycleAcceptance::Already,
        ),
    ] {
        let unanswered = LifecycleAction {
            attempt: attempt.map(str::to_owned),
            observed: Some(observation(kind)),
            ..action(true)
        };
        assert!(!unanswered.is_pending());
        let stopped = SessionLifecycle {
            stop: Some(unanswered.clone()),
            ..SessionLifecycle::default()
        };
        assert_eq!(stopped.accept(Stop), expected, "{attempt:?} {kind:?}");
        assert_eq!(stopped.pending(), None);
        let killed = SessionLifecycle {
            kill: Some(unanswered),
            ..SessionLifecycle::default()
        };
        assert_eq!(killed.accept(Kill), expected, "{attempt:?} {kind:?}");
    }
}

#[test]
fn a_pending_kill_comes_before_a_pending_stop() {
    use LifecycleActionKind::*;
    assert_eq!(lifecycle(Some(true), Some(true)).pending(), Some(Kill));
    assert_eq!(lifecycle(Some(true), Some(false)).pending(), Some(Stop));
    assert_eq!(lifecycle(Some(false), Some(false)).pending(), None);
    assert_eq!(lifecycle(None, None).pending(), None);
}

#[test]
fn an_action_on_an_unreachable_session_settles_on_acceptance() {
    let running = observation(ObservationKind::Running);
    let action = LifecycleAction::accept(LATER, Some("session_2"), Some(&running));
    assert!(action.is_pending());
    assert_eq!(action.accepted, LATER);
    assert_eq!(action.requester_session_id.as_deref(), Some("session_2"));
    assert!(LifecycleAction::accept(LATER, None, None).is_pending());

    for kind in [ObservationKind::Lost, ObservationKind::Mismatched] {
        let observed = observation(kind);
        let action = LifecycleAction::accept(LATER, None, Some(&observed));
        assert!(!action.is_pending());
        assert_eq!(action.attempt.as_deref(), Some("nothing to kill"));
        assert_eq!(action.observed, Some(observed));
    }
}

#[test]
fn a_schedule_patch_stamps_the_enabled_transitions() {
    let pause = SchedulePatch {
        enabled: Some(false),
        ..SchedulePatch::default()
    };
    let paused = schedule(true).apply_patch(&pause, LATER);
    assert!(!paused.enabled);
    assert_eq!(paused.paused_at.as_deref(), Some(LATER));
    assert_eq!(paused.enabled_at.as_deref(), Some(AT));

    let resume = SchedulePatch {
        enabled: Some(true),
        ..SchedulePatch::default()
    };
    let resumed = paused
        .clone()
        .apply_patch(&resume, "2026-09-26T02:00:00.000Z");
    assert!(resumed.enabled);
    assert_eq!(resumed.paused_at.as_deref(), Some(LATER));
    assert_eq!(
        resumed.enabled_at.as_deref(),
        Some("2026-09-26T02:00:00.000Z")
    );

    let retime = SchedulePatch {
        cron: Some("5 * * * *".to_owned()),
        ..SchedulePatch::default()
    };
    let retimed = resumed.clone().apply_patch(&retime, LATER);
    assert_eq!(retimed.cron, "5 * * * *");
    assert_eq!(retimed.timezone, "UTC");
    assert_eq!(retimed.enabled_at, resumed.enabled_at);
    assert_eq!(retimed.created_at, AT);
}

#[test]
fn a_schedule_patch_creates_a_schedule_only_with_a_cron() {
    let default = SchedulePatch::default();
    assert_eq!(Schedule::from_patch(&default, AT, "Asia/Tokyo"), None);

    let patch = SchedulePatch {
        cron: Some("0 * * * *".to_owned()),
        enabled: Some(false),
        ..SchedulePatch::default()
    };
    let created = Schedule::from_patch(&patch, AT, "Asia/Tokyo").unwrap();
    assert_eq!(created.timezone, "Asia/Tokyo");
    assert!(!created.enabled);
    assert_eq!(created.created_at, AT);
    assert_eq!(created.paused_at.as_deref(), Some(AT));
    assert_eq!(created.enabled_at, None);
}

#[test]
fn a_declaration_keeps_the_existing_enabled_state() {
    let declaration = ScheduleDeclaration {
        cron: "5 * * * *".to_owned(),
        timezone: "Asia/Tokyo".to_owned(),
    };
    let fresh = Schedule::declare(&declaration, None, LATER);
    assert!(fresh.enabled);
    assert_eq!(fresh.enabled_at.as_deref(), Some(LATER));
    assert_eq!(fresh.paused_at, None);

    let paused = Schedule::declare(&declaration, Some(schedule(false)), LATER);
    assert!(!paused.enabled);
    assert_eq!(paused.cron, "5 * * * *");
    assert_eq!(paused.timezone, "Asia/Tokyo");
    assert_eq!(paused.created_at, AT);
    assert_eq!(paused.paused_at.as_deref(), Some(AT));
}

#[test]
fn a_schedule_fires_a_tick_only_while_unchanged_and_enabled() {
    let expected = schedule(true);
    assert!(expected.fires_as(&expected));
    assert!(!schedule(false).fires_as(&expected));
    let revised = Schedule {
        revision: expected.revision + 1,
        ..expected.clone()
    };
    assert!(!revised.fires_as(&expected));
    let invalid = Schedule {
        cron: "61 * * * *".to_owned(),
        ..expected.clone()
    };
    assert!(expected.validate().is_ok());
    assert!(invalid.validate().is_err());
}

#[test]
fn a_launch_spec_supports_the_kind_that_it_runs() {
    let job = LaunchSpec {
        command: Some(vec!["true".to_owned()]),
        ..LaunchSpec::default()
    };
    let agent = LaunchSpec {
        runtime: Some("claude-code".to_owned()),
        ..LaunchSpec::default()
    };
    assert!(job.supports(ActorKind::Job));
    assert!(!job.supports(ActorKind::Agent));
    assert!(agent.supports(ActorKind::Agent));
    assert!(!agent.supports(ActorKind::Job));
    assert!(job.validate_kind(ActorKind::Job).is_ok());
    assert!(job.validate_kind(ActorKind::Agent).is_err());
    assert!(agent.validate_kind(ActorKind::Agent).is_ok());
    assert!(agent.validate_kind(ActorKind::Job).is_err());
}

#[test]
fn only_the_first_generation_starts_an_actor() {
    assert_eq!(StartKind::for_generation(1), StartKind::Start);
    assert_eq!(StartKind::for_generation(2), StartKind::Replacement);
}

#[test]
fn installed_plugins_decide_the_refusal() {
    struct Installed(bool);
    impl InstalledPlugins for Installed {
        fn has_runtime(&self, name: &str) -> bool {
            self.0 && name == "claude-code"
        }
        fn has_provider(&self, name: &str) -> bool {
            self.0 && name == "tmux"
        }
        fn has_any(&self, _: PluginKind) -> bool {
            self.0
        }
        fn runtime_owned_settings(&self, _: &str) -> Vec<String> {
            vec!["hooks".to_owned()]
        }
    }
    let launch = LaunchSpec {
        runtime: Some("claude-code".to_owned()),
        provider: Some("tmux".to_owned()),
        ..LaunchSpec::default()
    };
    assert_eq!(launch.validate_installed(&Installed(true)), Ok(()));
    assert_eq!(
        launch.validate_installed(&Installed(false)),
        Err(InstallRefusal::NoneInstalled(PluginKind::Runtime))
    );
    let missing = LaunchSpec {
        provider: Some("herdr".to_owned()),
        ..launch.clone()
    };
    assert_eq!(
        missing.validate_installed(&Installed(true)),
        Err(InstallRefusal::NotInstalled {
            kind: PluginKind::Provider,
            name: "herdr".to_owned(),
        })
    );
    let owned = LaunchSpec {
        runtime_settings: Some(json!({ "hooks": {} })),
        ..launch
    };
    assert_eq!(
        owned.validate_installed(&Installed(true)),
        Err(InstallRefusal::OwnedSetting {
            runtime: "claude-code".to_owned(),
            key: "hooks".to_owned(),
        })
    );
}

#[test]
fn a_launch_spec_takes_env_names_a_shell_can_assign() {
    let launch = |name: &str| LaunchSpec {
        command: Some(vec!["true".to_owned()]),
        workdir: Some("/tmp".to_owned()),
        env: Some([(name.to_owned(), "value".to_owned())].into()),
        provider: Some("tmux".to_owned()),
        node: Some("local".to_owned()),
        ..LaunchSpec::default()
    };
    assert_eq!(launch("GH_CONFIG_DIR").validate(), Ok(()));
    for name in ["", "1PATH", "A-B", "A=B"] {
        assert!(launch(name).validate().is_err(), "{name:?}");
    }

    // An update replaces the declared environment as a whole.
    let patch: LaunchSpecPatch = serde_json::from_value(json!({"env": {"B": "2"}})).unwrap();
    assert_eq!(
        launch("A").apply_patch(&patch).env,
        Some([("B".to_owned(), "2".to_owned())].into())
    );
    let dropped: LaunchSpecPatch = serde_json::from_value(json!({"env": null})).unwrap();
    assert_eq!(launch("A").apply_patch(&dropped).env, None);
}

#[test]
fn a_launch_spec_trusts_the_workdir_only_for_a_runtime() {
    let launch = |runtime: bool| LaunchSpec {
        runtime: runtime.then(|| "codex".to_owned()),
        command: (!runtime).then(|| vec!["true".to_owned()]),
        trust_workdir: Some(true),
        workdir: Some("/tmp".to_owned()),
        provider: Some("tmux".to_owned()),
        node: Some("local".to_owned()),
        ..LaunchSpec::default()
    };
    assert_eq!(launch(true).validate(), Ok(()));
    assert!(launch(false).validate().is_err());

    let dropped: LaunchSpecPatch = serde_json::from_value(json!({"trustWorkdir": null})).unwrap();
    assert_eq!(launch(true).apply_patch(&dropped).trust_workdir, None);
}

#[test]
fn an_actor_marks_mail_and_skips_against_its_current_session() {
    let mut running = actor(vec![session(AT, None)], 2);
    running.derive(Some(LATER));
    assert!(running.current_session.is_some());
    assert!(running.unread_mail);
    assert!(running.skip_mark);

    // A tick skipped before the current Session started does not mark it.
    let mut restarted = actor(vec![session(LATER, None)], 0);
    restarted.derive(Some(AT));
    assert!(!restarted.unread_mail);
    assert!(!restarted.skip_mark);

    let mut ended = actor(vec![session(AT, Some(LATER))], 2);
    ended.derive(Some(LATER));
    assert!(ended.current_session.is_none());
    assert_eq!(
        ended.latest_session.as_ref().map(|s| s.outcome),
        Some(Outcome::Succeeded)
    );
    assert!(!ended.unread_mail);
    assert!(!ended.skip_mark);
}

fn runtime_session(end: Option<&str>, ownership: Ownership, model: Option<Value>) -> Session {
    let mut session = session(AT, end);
    session.execution.execution_type = "agent-runtime".to_owned();
    session.ownership = ownership;
    session.model = model;
    session
}

#[test]
fn an_actor_marks_an_owned_runtime_its_runtime_has_not_linked_to() {
    let mut unlinked = actor(vec![runtime_session(None, Ownership::Owned, None)], 0);
    unlinked.derive(None);
    assert!(unlinked.unlinked_mark);

    let identified = json!(["Claude Code", null]);
    let linked_cases = [
        runtime_session(None, Ownership::Owned, Some(identified)),
        // An attached Runtime is linked by `feld actor attach --self`, not at its start.
        runtime_session(None, Ownership::Attached, None),
        runtime_session(Some(LATER), Ownership::Owned, None),
        session(AT, None),
    ];
    for case in linked_cases {
        let mut linked = actor(vec![case], 0);
        linked.derive(None);
        assert!(!linked.unlinked_mark);
    }
}

#[test]
fn requesters_name_themselves() {
    let actor = Requester::Actor {
        id: "actor_1".to_owned(),
        workspace: "default".to_owned(),
        name: "dev".to_owned(),
    };
    assert_eq!(actor.name(), "dev");
    assert_eq!(actor.actor_id(), Some("actor_1"));
    assert_eq!(Requester::Admin.name(), "Admin");
    assert_eq!(Requester::Admin.actor_id(), None);
}
