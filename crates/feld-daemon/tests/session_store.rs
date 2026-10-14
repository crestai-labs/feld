use std::collections::HashSet;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use feld_core::model::RuntimeIdentity;
use feld_core::model::{
    ActorKind, EndKind, Execution, Grant, LaunchSpec, ObservationKind, Outcome, Ownership,
    Placement, Record, RecordKind, Requester, StartKind,
};
use feld_daemon::store::{
    AcceptedLifecycleAction, ActorPatch, ActorWriteError, LifecycleActionKind, MessageWriteError,
    ObservationUpdate, RecordFilter, RequestIdentity, SessionWriteError, Store, Writer,
};
use serde_json::json;

/// The records of one Session, of one kind when `kind` is given.
fn records_of_session(store: &Store, session_id: &str, kind: Option<RecordKind>) -> Vec<Record> {
    store
        .list_records(&RecordFilter {
            session: Some(session_id),
            kind,
            ..RecordFilter::default()
        })
        .unwrap()
}

fn observations(store: &Store, session_id: &str) -> Vec<Record> {
    records_of_session(store, session_id, Some(RecordKind::Observation))
}

fn spawn_records(store: &Store, start: Option<&str>, end: Option<&str>) -> Vec<Record> {
    store
        .list_records(&RecordFilter {
            workspace: Some("default"),
            kind: Some(RecordKind::Spawned),
            start,
            end,
            ..RecordFilter::default()
        })
        .unwrap()
}

fn launch(workdir: &str) -> LaunchSpec {
    LaunchSpec {
        runtime: None,
        runtime_settings: None,
        trust_workdir: None,
        command: Some(vec!["sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()]),
        workdir: Some(workdir.to_owned()),
        env: None,
        provider: Some("tmux".to_owned()),
        node: Some("local".to_owned()),
        provider_options: Some(json!({"socket": "feld-test", "unit": "session"})),
    }
}

fn attached_execution() -> feld_daemon::store::AttachedExecution<'static> {
    feld_daemon::store::AttachedExecution {
        runtime: "claude-code",
        label: "Claude Code",
    }
}

fn execution() -> Execution {
    Execution {
        runtime: None,
        execution_type: "program".to_owned(),
        label: "Shell program".to_owned(),
        command: Some(vec!["sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()]),
        ai: false,
        telemetry_collector_available: None,
        telemetry_runtime_prepared: None,
    }
}

fn placement(workdir: &str) -> Placement {
    Placement {
        provider: "tmux".to_owned(),
        node: "local".to_owned(),
        workdir: workdir.to_owned(),
        placement: json!({
            "endpoint": {"kind": "name", "value": "feld-test"},
            "unit": "session"
        }),
    }
}

fn observation(kind: ObservationKind, at: &str, exit_code: Option<i32>) -> ObservationUpdate {
    ObservationUpdate {
        kind,
        at: at.to_owned(),
        detail: None,
        exit_code,
    }
}

fn attached_placement(runtime_pid: u32) -> Placement {
    Placement {
        provider: "tmux".to_owned(),
        node: "local".to_owned(),
        workdir: "/tmp".to_owned(),
        placement: json!({
            "endpoint": {"kind": "path", "value": "/tmp/feld-attached.sock"},
            "unit": "pane",
            "borrowed": "@1",
            "workdir": "/tmp",
            "server": {"pid": 100, "startTime": "server"},
            "sessionId": "$1",
            "sessionCreatedAt": 1,
            "sessionOrigin": "existing",
            "windowId": "@1",
            "windowOrigin": "existing",
            "paneId": "%1",
            "paneOrigin": "existing",
            "paneProcess": {"pid": 101, "startTime": "shell"},
            "process": {"pid": runtime_pid, "startTime": "runtime"}
        }),
    }
}

/// `startFailed` is the record that the Provider made no execution, so only a launch that
/// proved it (`gone`) writes it. A launch the Provider could not account for leaves the
/// Session's ordinary reading, and a later `running` still settles its start.
#[test]
fn only_a_launch_proven_to_have_made_nothing_records_that_the_start_failed() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/launch-failure");
    let admit = |actor: &str| {
        store
            .create_actor("default", actor, ActorKind::Job, &launch, None, None)
            .unwrap();
        store
            .admit_start(
                "default",
                actor,
                &launch,
                &execution(),
                &placement("/tmp/launch-failure"),
                RequestIdentity {
                    requester: &Requester::Admin,
                    session_id: None,
                },
            )
            .unwrap()
    };

    let proven = admit("proven");
    let closed = store
        .record_launch_failure(
            &proven.session.id,
            &observation(ObservationKind::Gone, "2026-09-16T00:00:01Z", None),
        )
        .unwrap();
    assert!(closed.lifecycle.start_failed.is_some());
    assert!(closed.lifecycle.start_settled.is_none());
    assert_eq!(closed.end_kind, EndKind::Exited);

    // A Provider that cannot say what it left behind answers `lost`, which core records as
    // an ordinary observation; the Session stays open and a later `running` settles it.
    let unproven = admit("unproven");
    let open = store
        .record_observation(
            &unproven.session.id,
            &observation(ObservationKind::Lost, "2026-09-16T00:00:01Z", None),
            None,
        )
        .unwrap();
    assert!(open.lifecycle.start_failed.is_none());
    assert!(open.end.is_none());
    let settled = store
        .record_observation(
            &unproven.session.id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:02Z", None),
            None,
        )
        .unwrap();
    assert!(settled.lifecycle.start_failed.is_none());
    assert_eq!(
        settled.lifecycle.start_settled.as_deref(),
        Some("2026-09-16T00:00:02Z")
    );
}

#[test]
fn committed_mutations_publish_exact_resource_ids_and_workspaces() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let mut events = store.subscribe_events();
    let launch = launch("/tmp/event-store");
    store
        .create_actor("default", "worker", ActorKind::Job, &launch, None, None)
        .unwrap();
    let pending = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/event-store"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    store
        .record_observation(
            &pending.session.id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:01Z", None),
            None,
        )
        .unwrap();
    let message = store
        .send_message(
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
            None,
            "default",
            "worker",
            None,
            "hello",
        )
        .unwrap();
    assert!(matches!(
        store
            .record_telemetry(
                "request-exact",
                &feld_core::telemetry::telemetry_token(&pending.token),
                "metrics",
                "application/x-protobuf",
                b"payload",
            )
            .unwrap(),
        feld_daemon::store::TelemetryAdmission::Stored { .. }
    ));
    store
        .read_mailbox(
            &pending.session.id,
            &pending.token,
            std::slice::from_ref(&message.id),
        )
        .unwrap();

    let mut published = Vec::new();
    while let Ok(event) = events.try_recv() {
        if event.resource == "quota" {
            assert_eq!(event.workspace, None);
        } else {
            assert_eq!(event.workspace.as_deref(), Some("default"));
        }
        published.push((event.resource, event.id));
    }
    for expected in [
        ("actor", "worker"),
        ("session", pending.session.id.as_str()),
        ("record", pending.session.id.as_str()),
        ("message", message.id.as_str()),
        ("quota", "request-exact"),
    ] {
        assert!(
            published
                .iter()
                .any(|(resource, id)| resource == expected.0 && id == expected.1),
            "missing event {expected:?} in {published:?}"
        );
    }
    assert!(!published.iter().any(|(_, id)| id == "worker/messages"));
}

/// A Message a Runtime holds in a queue of its own cannot be taken back, so it is no longer
/// Feld's to delete; it waits for its read all the same. A Message deleted before it was
/// queued is neither queued nor read.
#[test]
fn a_queued_message_waits_and_refuses_its_delete() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/queued-message");
    store
        .create_actor("default", "worker", ActorKind::Job, &launch, None, None)
        .unwrap();
    let pending = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/queued-message"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let session = pending.session.id.as_str();
    let send = |body: &str| {
        store
            .send_message(
                RequestIdentity {
                    requester: &Requester::Admin,
                    session_id: None,
                },
                None,
                "default",
                "worker",
                None,
                body,
            )
            .unwrap()
            .id
    };
    let queued = send("queued");
    let deleted = send("deleted");

    let first = store
        .queue_mailbox(session, &pending.token, std::slice::from_ref(&queued))
        .unwrap();
    assert_eq!(first[0].session_id, session);
    // Queueing it again keeps what the first queueing recorded.
    let again = store
        .queue_mailbox(session, &pending.token, std::slice::from_ref(&queued))
        .unwrap();
    assert_eq!(again, first);
    let message = store.get_message(&queued).unwrap().unwrap();
    assert!(message.waiting);
    assert_eq!(message.queued.as_ref(), first.first());
    assert!(matches!(
        store.delete_message(&queued),
        Err(MessageWriteError::Queued(id)) if id == queued
    ));
    assert_eq!(
        store.mailbox(session, &pending.token, None).unwrap().len(),
        2
    );

    store.delete_message(&deleted).unwrap();
    assert!(matches!(
        store.queue_mailbox(session, &pending.token, std::slice::from_ref(&deleted)),
        Err(MessageWriteError::MessageNotFound(id)) if id == deleted
    ));
    assert!(matches!(
        store.read_mailbox(session, &pending.token, std::slice::from_ref(&deleted)),
        Err(MessageWriteError::MessageNotFound(id)) if id == deleted
    ));
    let deleted = store.get_message(&deleted).unwrap().unwrap();
    assert!(deleted.queued.is_none() && deleted.read.is_none());

    // A queueing the Runtime refused is taken back, and the Message is deletable again.
    let refused = send("refused");
    store
        .queue_mailbox(session, &pending.token, std::slice::from_ref(&refused))
        .unwrap();
    let taken_back = store
        .unqueue_mailbox(session, &pending.token, &refused)
        .unwrap();
    assert!(taken_back.queued.is_none() && taken_back.waiting);
    store.delete_message(&refused).unwrap();

    store
        .read_mailbox(session, &pending.token, std::slice::from_ref(&queued))
        .unwrap();
    let message = store.get_message(&queued).unwrap().unwrap();
    assert!(!message.waiting);
    assert!(message.queued.is_some());
    // A read Message keeps its queueing.
    let kept = store
        .unqueue_mailbox(session, &pending.token, &queued)
        .unwrap();
    assert!(kept.queued.is_some());

    // The queue of a Runtime ends with its Session: what it held unread waits for the next
    // Session as a plain waiting Message, and can be deleted.
    let stranded = send("stranded");
    store
        .queue_mailbox(session, &pending.token, std::slice::from_ref(&stranded))
        .unwrap();
    store
        .record_observation(
            session,
            &observation(ObservationKind::Gone, "2026-09-16T00:00:03Z", Some(0)),
            None,
        )
        .unwrap();
    assert!(store.get_session(session).unwrap().unwrap().end.is_some());
    let stranded_message = store.get_message(&stranded).unwrap().unwrap();
    assert!(stranded_message.queued.is_none() && stranded_message.waiting);
    assert!(
        store
            .get_message(&queued)
            .unwrap()
            .unwrap()
            .queued
            .is_some()
    );
    store.delete_message(&stranded).unwrap();
}

/// A Session replaced from `lost` or `mismatched` closes as `unknown`, and its Runtime may
/// run on with its queue: the Messages it queued stay queued, and so not deletable, until
/// a read. The next Session reads them from the mailbox all the same.
#[test]
fn a_session_closed_as_unknown_keeps_what_it_queued() {
    for kind in [ObservationKind::Lost, ObservationKind::Mismatched] {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::open(&temporary.path().join("feld.db")).unwrap();
        let launch = launch("/tmp/unknown-queue");
        store
            .create_actor("default", "worker", ActorKind::Job, &launch, None, None)
            .unwrap();
        let admit = || {
            store
                .admit_start(
                    "default",
                    "worker",
                    &launch,
                    &execution(),
                    &placement("/tmp/unknown-queue"),
                    RequestIdentity {
                        requester: &Requester::Admin,
                        session_id: None,
                    },
                )
                .unwrap()
        };
        let first = admit();
        let message = store
            .send_message(
                RequestIdentity {
                    requester: &Requester::Admin,
                    session_id: None,
                },
                None,
                "default",
                "worker",
                None,
                "held",
            )
            .unwrap()
            .id;
        let queued = store
            .queue_mailbox(
                &first.session.id,
                &first.token,
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .record_observation(
                &first.session.id,
                &observation(kind, "2026-09-16T00:00:01Z", None),
                None,
            )
            .unwrap();
        let second = admit();
        let closed = store.get_session(&first.session.id).unwrap().unwrap();
        assert_eq!(closed.end_kind, EndKind::Unknown, "{kind:?}");

        let held = store.get_message(&message).unwrap().unwrap();
        assert_eq!(held.queued.as_ref(), queued.first(), "{kind:?}");
        assert!(held.waiting);
        assert!(matches!(
            store.delete_message(&message),
            Err(MessageWriteError::Queued(_))
        ));
        // The next Session is offered it, and queueing it keeps the first queueing.
        let mailbox = store
            .mailbox(&second.session.id, &second.token, None)
            .unwrap();
        assert_eq!(mailbox.len(), 1);
        let again = store
            .queue_mailbox(
                &second.session.id,
                &second.token,
                std::slice::from_ref(&message),
            )
            .unwrap();
        assert_eq!(again, queued);
        store
            .read_mailbox(
                &second.session.id,
                &second.token,
                std::slice::from_ref(&message),
            )
            .unwrap();
        assert!(!store.get_message(&message).unwrap().unwrap().waiting);
    }
}

#[test]
fn repeated_observations_of_the_same_kind_only_advance_the_timestamp_not_since() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/quiet-observations");
    store
        .create_actor("default", "worker", ActorKind::Job, &launch, None, None)
        .unwrap();
    let pending = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/quiet-observations"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let session_id = pending.session.id.as_str();
    store
        .record_observation(
            session_id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:01Z", None),
            None,
        )
        .unwrap();
    let mut events = store.subscribe_events();
    let session = store
        .record_observation(
            session_id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:02Z", None),
            None,
        )
        .unwrap();
    assert_eq!(
        session.observation.as_ref().unwrap().at,
        "2026-09-16T00:00:02Z"
    );
    assert_eq!(
        session.observation.as_ref().unwrap().since.as_deref(),
        Some("2026-09-16T00:00:01Z")
    );
    assert!(
        events.try_recv().is_err(),
        "re-observation published events"
    );
    assert_eq!(observations(&store, session_id).len(), 1);
    // A Placement write is a change readers must see, even without a record.
    store
        .record_observation(
            session_id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:02Z", None),
            Some(&placement("/tmp/quiet-observations")),
        )
        .unwrap();
    assert_eq!(events.try_recv().unwrap().resource, "session");
    while events.try_recv().is_ok() {}
    assert_eq!(observations(&store, session_id).len(), 1);
    assert_eq!(
        records_of_session(&store, session_id, None)
            .iter()
            .filter(|record| record.kind == RecordKind::Observation)
            .count(),
        1
    );

    let session = store
        .record_observation(
            session_id,
            &observation(ObservationKind::Gone, "2026-09-16T00:00:03Z", Some(0)),
            None,
        )
        .unwrap();
    assert_eq!(
        session.observation.as_ref().unwrap().since.as_deref(),
        Some("2026-09-16T00:00:03Z")
    );
    let mut published = Vec::new();
    while let Ok(event) = events.try_recv() {
        published.push(event.resource);
    }
    for resource in ["session", "record"] {
        assert!(
            published.iter().any(|published| published == resource),
            "missing {resource} event in {published:?}"
        );
    }
    assert_eq!(observations(&store, session_id).len(), 2);
}

#[test]
fn message_console_events_cover_both_sessions_once() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/message-events");
    let sender_id = store
        .create_actor("default", "sender", ActorKind::Job, &launch, None, None)
        .unwrap()
        .id;
    store
        .create_actor("default", "recipient", ActorKind::Job, &launch, None, None)
        .unwrap();
    let sender = store
        .admit_start(
            "default",
            "sender",
            &launch,
            &execution(),
            &placement("/tmp/message-events"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let recipient = store
        .admit_start(
            "default",
            "recipient",
            &launch,
            &execution(),
            &placement("/tmp/message-events"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let mut events = store.subscribe_events();
    store
        .send_message(
            RequestIdentity {
                requester: &Requester::Actor {
                    id: sender_id.clone(),
                    workspace: "default".to_owned(),
                    name: "sender".to_owned(),
                },
                session_id: Some(&sender.session.id),
            },
            Some(&sender.token),
            "default",
            "recipient",
            None,
            "cross-session",
        )
        .unwrap();
    let cross: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert_eq!(
        cross
            .iter()
            .filter(|event| event.resource == "record" && event.id == sender.session.id)
            .count(),
        1
    );
    assert_eq!(
        cross
            .iter()
            .filter(|event| event.resource == "record" && event.id == recipient.session.id)
            .count(),
        1
    );

    store
        .send_message(
            RequestIdentity {
                requester: &Requester::Actor {
                    id: sender_id.clone(),
                    workspace: "default".to_owned(),
                    name: "sender".to_owned(),
                },
                session_id: Some(&sender.session.id),
            },
            Some(&sender.token),
            "default",
            "sender",
            None,
            "self-message",
        )
        .unwrap();
    let own: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert_eq!(
        own.iter()
            .filter(|event| event.resource == "record" && event.id == sender.session.id)
            .count(),
        1
    );
}

#[test]
fn replacement_notifies_the_closed_and_new_sessions_after_commit() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/replacement-events");
    store
        .create_actor("default", "worker", ActorKind::Job, &launch, None, None)
        .unwrap();
    let first = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/replacement-events"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    store
        .record_observation(
            &first.session.id,
            &observation(ObservationKind::Lost, "2026-09-16T00:00:01Z", None),
            None,
        )
        .unwrap();
    let mut events = store.subscribe_events();
    let replacement = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/replacement-events"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    assert_eq!(
        replacement
            .replaced_session
            .as_ref()
            .map(|session| &session.id),
        Some(&first.session.id)
    );
    let published: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    for id in [&first.session.id, &replacement.session.id] {
        assert!(published.iter().any(|event| {
            event.resource == "session"
                && event.id == *id
                && event.workspace.as_deref() == Some("default")
        }));
        assert!(published.iter().any(|event| {
            event.resource == "record"
                && event.id == *id
                && event.workspace.as_deref() == Some("default")
        }));
    }
}

#[test]
fn attached_sessions_remember_a_runtime_started_before_telemetry_preparation() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let placement = attached_placement(103);
    let first = store
        .admit_attach(
            "default",
            "agent",
            &placement,
            attached_execution(),
            true,
            false,
        )
        .unwrap();
    assert_eq!(
        first.pending.session.execution.telemetry_runtime_prepared,
        Some(false)
    );
    let rotated = store
        .admit_attach(
            "default",
            "agent",
            &placement,
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    assert!(rotated.reattached);
    assert_eq!(
        rotated.pending.session.execution.telemetry_runtime_prepared,
        Some(false)
    );
    let stored = store
        .get_session(&first.pending.session.id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.execution.telemetry_runtime_prepared, Some(false));
    assert_eq!(stored.execution.telemetry_collector_available, Some(true));

    let prepared = store
        .admit_attach(
            "default",
            "prepared",
            &attached_placement(104),
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    assert_eq!(
        prepared
            .pending
            .session
            .execution
            .telemetry_runtime_prepared,
        Some(true)
    );
}

#[test]
fn attached_sessions_are_atomic_rotate_tokens_and_record_runtime_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let placement = attached_placement(102);
    let first = store
        .admit_attach(
            "default",
            "agent",
            &placement,
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    assert!(!first.reattached);
    assert_eq!(first.pending.session.start_kind, StartKind::Attach);
    assert_eq!(first.pending.session.ownership, Ownership::Attached);
    assert_eq!(
        first.pending.session.observation.as_ref().unwrap().kind,
        ObservationKind::Running
    );
    assert!(first.pending.session.lifecycle.start_settled.is_some());
    assert!(
        store
            .get_actor("default", "agent")
            .unwrap()
            .unwrap()
            .launch_spec
            .is_none()
    );
    assert!(spawn_records(&store, None, None).is_empty());

    let second = store
        .admit_attach(
            "default",
            "agent",
            &placement,
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    assert!(second.reattached);
    assert_eq!(second.pending.session.id, first.pending.session.id);
    assert_ne!(second.pending.token, first.pending.token);
    assert!(
        store
            .authenticate_session(&first.pending.token)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .authenticate_session(&second.pending.token)
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        store.admit_attach(
            "default",
            "other",
            &placement,
            attached_execution(),
            true,
            true
        ),
        Err(SessionWriteError::RuntimeAlreadyAttached { .. })
    ));
    assert!(matches!(
        store.accept_lifecycle_action(&first.pending.session.id, LifecycleActionKind::Stop, None),
        Err(SessionWriteError::NotOwned(_))
    ));

    let identity = RuntimeIdentity {
        session_id: "runtime-1".to_owned(),
        transcript_path: "/tmp/transcript.jsonl".to_owned(),
        model: json!("claude-test"),
    };
    assert!(matches!(
        store.record_runtime_identity(&first.pending.session.id, &first.pending.token, &identity,),
        Err(SessionWriteError::InvalidToken)
    ));
    let identified = store
        .record_runtime_identity(&first.pending.session.id, &second.pending.token, &identity)
        .unwrap();
    assert_eq!(
        identified.model,
        Some(json!(["Claude Code", "claude-test"]))
    );
    assert_eq!(
        store
            .record_runtime_identity(&first.pending.session.id, &second.pending.token, &identity,)
            .unwrap()
            .model,
        identified.model
    );
    // A deliverer that does not know the model (the attach CLI) leaves the recorded one;
    // a hook that reports the Runtime's current model moves it; another Runtime conflicts.
    let unknown_model = RuntimeIdentity {
        model: json!(null),
        ..identity.clone()
    };
    assert_eq!(
        store
            .record_runtime_identity(
                &first.pending.session.id,
                &second.pending.token,
                &unknown_model,
            )
            .unwrap()
            .model,
        identified.model
    );
    let switched_model = RuntimeIdentity {
        model: json!("claude-other"),
        ..identity.clone()
    };
    assert_eq!(
        store
            .record_runtime_identity(
                &first.pending.session.id,
                &second.pending.token,
                &switched_model,
            )
            .unwrap()
            .model,
        Some(json!(["Claude Code", "claude-other"]))
    );
    let other_runtime = RuntimeIdentity {
        transcript_path: "/tmp/other.jsonl".to_owned(),
        ..identity.clone()
    };
    assert!(matches!(
        store.record_runtime_identity(
            &first.pending.session.id,
            &second.pending.token,
            &other_runtime,
        ),
        Err(SessionWriteError::RuntimeIdentityConflict {
            ref session,
            ref recorded_transcript_path,
            ref requested_transcript_path,
            ..
        }) if *session == first.pending.session.id
            && recorded_transcript_path == &identity.transcript_path
            && requested_transcript_path == "/tmp/other.jsonl"
    ));

    let ended = store
        .record_observation(
            &first.pending.session.id,
            &observation(ObservationKind::Gone, "2026-09-16T00:03:00Z", None),
            None,
        )
        .unwrap();
    assert_eq!(ended.outcome, Outcome::Ended);
    assert_eq!(ended.exit_code, None);
    assert!(
        store
            .authenticate_session(&second.pending.token)
            .unwrap()
            .is_none()
    );
}

#[test]
fn attach_refuses_a_missing_workspace_and_a_job() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let placement = attached_placement(102);
    assert!(matches!(
        store.admit_attach(
            "nows",
            "agent",
            &placement,
            attached_execution(),
            true,
            true
        ),
        Err(SessionWriteError::WorkspaceNotFound(ref workspace)) if workspace == "nows"
    ));
    store
        .create_actor(
            "default",
            "job",
            ActorKind::Job,
            &launch("/tmp"),
            None,
            None,
        )
        .unwrap();
    assert!(matches!(
        store.admit_attach(
            "default",
            "job",
            &placement,
            attached_execution(),
            true,
            true
        ),
        Err(SessionWriteError::AttachToJob(ref actor)) if actor == "job"
    ));
}

#[test]
fn attach_refuses_the_runtime_an_owned_session_runs() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = LaunchSpec {
        runtime: Some("claude-code".to_owned()),
        command: None,
        ..launch("/tmp")
    };
    let execution = Execution {
        execution_type: "agent-runtime".to_owned(),
        runtime: Some("claude-code".to_owned()),
        label: "Claude Code".to_owned(),
        command: None,
        ai: true,
        ..execution()
    };
    store
        .create_actor("default", "owner", ActorKind::Agent, &launch, None, None)
        .unwrap();
    let mut launched = attached_placement(301);
    let fields = launched.placement.as_object_mut().unwrap();
    fields.remove("process");
    fields.insert(
        "program".to_owned(),
        json!({"pid": 301, "startTime": "runtime"}),
    );
    let owned = store
        .admit_start(
            "default",
            "owner",
            &launch,
            &execution,
            &launched,
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    store
        .record_observation(
            &owned.session.id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:01Z", None),
            None,
        )
        .unwrap();

    let runtime = attached_placement(301);
    assert!(matches!(
        store.admit_attach(
            "default",
            "other",
            &runtime,
            attached_execution(),
            true,
            true
        ),
        Err(SessionWriteError::RuntimeAlreadyAttached { ref actor, ref session })
            if actor == "owner" && *session == owned.session.id
    ));
    assert!(store.get_actor("default", "other").unwrap().is_none());
    assert!(matches!(
        store.admit_attach(
            "default",
            "owner",
            &runtime,
            attached_execution(),
            true,
            true
        ),
        Err(SessionWriteError::Admission { observation, .. }) if observation == "running"
    ));
    assert!(store.authenticate_session(&owned.token).unwrap().is_some());
}

#[test]
fn attach_replacement_notifies_the_closed_session() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let first = store
        .admit_attach(
            "default",
            "agent",
            &attached_placement(201),
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    store
        .record_observation(
            &first.pending.session.id,
            &observation(ObservationKind::Mismatched, "2026-09-16T00:01:00Z", None),
            None,
        )
        .unwrap();
    let mut events = store.subscribe_events();
    let second = store
        .admit_attach(
            "default",
            "agent",
            &attached_placement(202),
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    assert_ne!(second.pending.session.id, first.pending.session.id);
    assert_eq!(
        second
            .pending
            .replaced_session
            .as_ref()
            .map(|session| &session.id),
        Some(&first.pending.session.id)
    );
    let events: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    for id in [&first.pending.session.id, &second.pending.session.id] {
        assert!(
            events
                .iter()
                .any(|event| event.resource == "session" && event.id == *id)
        );
        assert!(
            events
                .iter()
                .any(|event| event.resource == "record" && event.id == *id)
        );
    }
}

#[test]
fn admission_generation_and_tokens_follow_observation_facts() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/session-store");
    store
        .create_actor("default", "worker", ActorKind::Job, &launch, None, None)
        .unwrap();

    let first = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/session-store"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    assert_eq!(first.session.generation, 1);
    assert_eq!(first.session.start_kind, StartKind::Start);
    assert!(store.authenticate_session(&first.token).unwrap().is_some());
    assert!(matches!(
        store.admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/session-store"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        ),
        Err(SessionWriteError::Admission { observation, .. }) if observation == "unavailable"
    ));

    store
        .record_observation(
            &first.session.id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:01Z", None),
            None,
        )
        .unwrap();
    store
        .record_observation(
            &first.session.id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:02Z", None),
            None,
        )
        .unwrap();
    assert!(matches!(
        store.admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/session-store"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        ),
        Err(SessionWriteError::Admission { observation, .. }) if observation == "running"
    ));
    let ended = store
        .record_observation(
            &first.session.id,
            &observation(ObservationKind::Gone, "2026-09-16T00:00:03Z", Some(0)),
            None,
        )
        .unwrap();
    assert_eq!(ended.end_kind, EndKind::Exited);
    assert_eq!(ended.outcome, Outcome::Succeeded);
    assert!(store.authenticate_session(&first.token).unwrap().is_none());

    let second = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/session-store"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    assert_eq!(second.session.generation, 2);
    assert_eq!(second.session.start_kind, StartKind::Replacement);
    store
        .record_observation(
            &second.session.id,
            &observation(ObservationKind::Running, "2026-09-16T00:00:04Z", None),
            None,
        )
        .unwrap();
    store
        .record_observation(
            &second.session.id,
            &observation(ObservationKind::Lost, "2026-09-16T00:00:05Z", None),
            None,
        )
        .unwrap();
    let third = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/session-store"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    assert_eq!(third.session.generation, 3);
    let replaced = store.get_session(&second.session.id).unwrap().unwrap();
    assert_eq!(replaced.end_kind, EndKind::Unknown);
    assert_eq!(replaced.outcome, Outcome::Unknown);
    assert!(store.authenticate_session(&second.token).unwrap().is_none());

    let history = observations(&store, &first.session.id);
    let kinds: Vec<ObservationKind> = history
        .iter()
        .map(|record| serde_json::from_value(record.data["kind"].clone()).unwrap())
        .collect();
    assert_eq!(kinds, [ObservationKind::Running, ObservationKind::Gone]);
    let spawns = spawn_records(&store, None, None);
    assert_eq!(spawns.len(), 3);
    assert!(spawns.iter().all(|spawn| {
        serde_json::from_value::<Requester>(spawn.data["spawner"].clone()).unwrap()
            == Requester::Admin
    }));

    let boundary = &spawns[0].at;
    assert!(spawn_records(&store, Some(boundary), Some(boundary)).is_empty());
    let before = (DateTime::parse_from_rfc3339(boundary).unwrap() - Duration::milliseconds(1))
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let after = (DateTime::parse_from_rfc3339(boundary).unwrap() + Duration::milliseconds(1))
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let left = spawn_records(&store, Some(&before), Some(boundary));
    let right = spawn_records(&store, Some(boundary), Some(&after));
    let whole = spawn_records(&store, Some(&before), Some(&after));
    let boundary_sessions: HashSet<_> = spawns
        .iter()
        .filter(|spawn| spawn.at == *boundary)
        .map(|spawn| spawn.session_id.clone())
        .collect();
    let left_sessions: HashSet<_> = left.iter().map(|spawn| spawn.session_id.clone()).collect();
    let right_sessions: HashSet<_> = right.iter().map(|spawn| spawn.session_id.clone()).collect();
    let whole_sessions: HashSet<_> = whole.iter().map(|spawn| spawn.session_id.clone()).collect();
    assert!(left_sessions.is_disjoint(&boundary_sessions));
    assert_eq!(right_sessions, boundary_sessions);
    assert!(left_sessions.is_disjoint(&right_sessions));
    assert_eq!(
        left_sessions
            .union(&right_sessions)
            .cloned()
            .collect::<HashSet<_>>(),
        whole_sessions
    );
}

#[test]
fn lifecycle_requests_record_attempts_and_close_sessions() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/lifecycle-store");
    store
        .create_actor("default", "runner", ActorKind::Job, &launch, None, None)
        .unwrap();
    let first = store
        .admit_start(
            "default",
            "runner",
            &launch,
            &execution(),
            &placement("/tmp/lifecycle-store"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    store
        .record_observation(
            &first.session.id,
            &observation(ObservationKind::Running, "2026-09-16T00:01:00Z", None),
            None,
        )
        .unwrap();
    assert!(matches!(
        store
            .accept_lifecycle_action(&first.session.id, LifecycleActionKind::Stop, None)
            .unwrap(),
        AcceptedLifecycleAction::NeedsProvider(_)
    ));
    assert!(matches!(
        store
            .accept_lifecycle_action(&first.session.id, LifecycleActionKind::Stop, None)
            .unwrap(),
        AcceptedLifecycleAction::Resume(_)
    ));
    // A Provider that could not answer leaves the Session open, and the same request
    // tries again.
    let unanswered = store
        .finish_lifecycle_action(
            &first.session.id,
            LifecycleActionKind::Stop,
            None,
            &observation(ObservationKind::Unknown, "2026-09-16T00:01:00Z", None),
        )
        .unwrap();
    assert!(unanswered.end.is_none());
    let stop = unanswered.lifecycle.stop.as_ref().unwrap();
    assert_eq!(stop.attempt, None);
    assert_eq!(
        stop.observed.as_ref().map(|observed| observed.kind),
        Some(ObservationKind::Unknown)
    );
    assert!(matches!(
        store
            .accept_lifecycle_action(&first.session.id, LifecycleActionKind::Stop, None)
            .unwrap(),
        AcceptedLifecycleAction::Resume(_)
    ));
    let stopped = store
        .finish_lifecycle_action(
            &first.session.id,
            LifecycleActionKind::Stop,
            Some("tmux sent SIGTERM to the pane's process"),
            &observation(ObservationKind::Gone, "2026-09-16T00:01:01Z", Some(143)),
        )
        .unwrap();
    assert_eq!(stopped.end_kind, EndKind::Stopped);
    assert_eq!(stopped.outcome, Outcome::Stopped);
    for action_kind in [LifecycleActionKind::Stop, LifecycleActionKind::Kill] {
        assert!(matches!(
            store.accept_lifecycle_action(&first.session.id, action_kind, None),
            Err(SessionWriteError::SessionClosed(_))
        ));
    }

    let second = store
        .admit_start(
            "default",
            "runner",
            &launch,
            &execution(),
            &placement("/tmp/lifecycle-store"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let lost = ObservationUpdate {
        detail: Some("the pane disappeared".to_owned()),
        ..observation(ObservationKind::Lost, "2026-09-16T00:02:00Z", None)
    };
    store
        .record_observation(&second.session.id, &lost, None)
        .unwrap();
    let closed = store
        .accept_lifecycle_action(&second.session.id, LifecycleActionKind::Kill, None)
        .unwrap();
    let AcceptedLifecycleAction::Completed(closed) = closed else {
        panic!("lost Session should close without a provider action");
    };
    assert_eq!(closed.end_kind, EndKind::Unknown);
    let kill = closed.lifecycle.kill.unwrap();
    assert_eq!(kill.attempt.as_deref(), Some("nothing to kill"));
    // The last observation stays as the Provider wrote it.
    let observed = kill.observed.unwrap();
    assert_eq!(observed.detail.as_deref(), Some("the pane disappeared"));
    assert_eq!(closed.observation.unwrap(), observed);
    for action_kind in [LifecycleActionKind::Stop, LifecycleActionKind::Kill] {
        assert!(matches!(
            store.accept_lifecycle_action(&second.session.id, action_kind, None),
            Err(SessionWriteError::SessionClosed(_))
        ));
    }

    let records = records_of_session(&store, &first.session.id, None);
    let kinds: Vec<_> = records.iter().map(|record| record.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "spawned",
            "session.started",
            "observation",
            "stop.requested",
            "observation",
            "observation",
            "session.ended"
        ]
    );
}

/// Records hold the Actor's `id`, so a Session, Message, and Spawn of a deleted Actor keep
/// resolving to it after a new Actor takes the same name.
#[test]
fn records_of_a_deleted_actor_keep_its_id_when_the_name_is_reused() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/reused-name");
    let old = store
        .create_actor("default", "worker", ActorKind::Job, &launch, None, None)
        .unwrap();
    let spawned = store
        .admit_start(
            "default",
            "worker",
            &launch,
            &execution(),
            &placement("/tmp/reused-name"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let message = store
        .send_message(
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
            None,
            "default",
            "worker",
            None,
            "before the delete",
        )
        .unwrap();
    store
        .record_observation(
            &spawned.session.id,
            &observation(ObservationKind::Gone, "2026-09-16T00:00:01Z", Some(0)),
            None,
        )
        .unwrap();
    store.delete_actor("default", "worker").unwrap();
    let new = store
        .create_actor("default", "worker", ActorKind::Job, &launch, None, None)
        .unwrap();
    assert_ne!(new.id, old.id);

    let session = store.get_session(&spawned.session.id).unwrap().unwrap();
    assert_eq!(session.actor_id, old.id);
    let message = store.get_message(&message.id).unwrap().unwrap();
    assert_eq!(message.to.id, old.id);
    let spawns = spawn_records(&store, None, None);
    assert_eq!(spawns.len(), 1);
    assert_eq!(spawns[0].actor.id, old.id);

    let deleted = store.get_actor_by_id(&old.id).unwrap().unwrap();
    assert!(deleted.deleted_at.is_some());
    assert_eq!(deleted.sessions.len(), 1);
    let live = store.get_actor("default", "worker").unwrap().unwrap();
    assert_eq!(live.id, new.id);
    assert!(live.deleted_at.is_none());
    assert!(live.sessions.is_empty());
}

#[test]
fn a_store_of_schema_5_keeps_its_session_tokens() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    let attach = || {
        store
            .admit_attach(
                "default",
                "agent",
                &attached_placement(102),
                attached_execution(),
                true,
                true,
            )
            .unwrap()
    };
    let first = attach();
    let second = attach();
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE sessions DROP COLUMN observation_since;
             DROP TABLE browser_tokens;
             ALTER TABLE sessions RENAME COLUMN token TO credential;
             ALTER TABLE telemetries RENAME COLUMN request_id TO id;
             ALTER TABLE telemetries RENAME TO telemetry_batches;
             DROP INDEX telemetries_session;
             CREATE INDEX telemetry_batches_session ON telemetry_batches(session_id, received_at, id);
             ALTER TABLE session_tokens RENAME COLUMN token TO credential;
             ALTER TABLE session_tokens RENAME TO session_credentials;
             ALTER TABLE token_secrets RENAME TO credential_secrets;
             ALTER TABLE messages DROP COLUMN queued_session_id;
             ALTER TABLE messages DROP COLUMN queued_at;
             ALTER TABLE messages DROP COLUMN label;
             ALTER TABLE actors DROP COLUMN grants;
             ALTER TABLE actors DROP COLUMN accept_from;
             ALTER TABLE sessions DROP COLUMN telemetry_bytes;
             PRAGMA user_version = 5;",
        )
        .unwrap();

    let store = Store::open(&path).unwrap();
    assert!(
        store
            .authenticate_session(&second.pending.token)
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .token_secrets()
            .unwrap()
            .contains(&first.pending.token)
    );
}

#[test]
fn a_store_of_schema_6_keeps_its_telemetry() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    let attached = store
        .admit_attach(
            "default",
            "agent",
            &attached_placement(102),
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    let record = |store: &Store, body: &[u8]| {
        store
            .record_telemetry(
                "request-before",
                &feld_core::telemetry::telemetry_token(&attached.pending.token),
                "metrics",
                "application/x-protobuf",
                body,
            )
            .unwrap()
    };
    record(&store, b"payload");
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE sessions DROP COLUMN observation_since;
             DROP TABLE browser_tokens;
             ALTER TABLE telemetries RENAME COLUMN request_id TO id;
             ALTER TABLE telemetries RENAME TO telemetry_batches;
             DROP INDEX telemetries_session;
             CREATE INDEX telemetry_batches_session ON telemetry_batches(session_id, received_at, id);
             ALTER TABLE messages DROP COLUMN queued_session_id;
             ALTER TABLE messages DROP COLUMN queued_at;
             ALTER TABLE messages DROP COLUMN label;
             ALTER TABLE actors DROP COLUMN grants;
             ALTER TABLE actors DROP COLUMN accept_from;
             ALTER TABLE sessions DROP COLUMN telemetry_bytes;
             PRAGMA user_version = 6;",
        )
        .unwrap();

    let store = Store::open(&path).unwrap();
    let stored = store.telemetries(&attached.pending.session.id).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].request_id, "request-before");
    assert_eq!(stored[0].body, b"payload");
    // The migration counts the stored Telemetry toward the Session limit.
    let bytes: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT telemetry_bytes FROM sessions WHERE id = ?1",
            [&attached.pending.session.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bytes, 7 + feld_daemon::store::TELEMETRY_ROW_OVERHEAD);
    assert!(matches!(
        record(&store, b"payload"),
        feld_daemon::store::TelemetryAdmission::Duplicate { .. }
    ));
    assert!(matches!(
        record(&store, b"other"),
        feld_daemon::store::TelemetryAdmission::Conflict
    ));
}

#[test]
fn a_store_of_schema_7_keeps_its_records_and_takes_missed_ticks() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    store
        .admit_attach(
            "default",
            "agent",
            &attached_placement(102),
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    let records = |store: &Store| {
        store
            .list_records(&feld_daemon::store::RecordFilter::default())
            .unwrap()
    };
    let before = records(&store);
    assert!(!before.is_empty());
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE sessions DROP COLUMN observation_since;
             DROP TABLE browser_tokens;
             CREATE TABLE records_7 (
               sequence INTEGER PRIMARY KEY AUTOINCREMENT,
               at TEXT NOT NULL,
               kind TEXT NOT NULL CHECK (kind IN ('session.started', 'session.ended', 'spawned',
                 'message.accepted', 'message.sent', 'message.read', 'stop.requested',
                 'kill.requested', 'observation', 'schedule.skipped')),
               actor_id TEXT NOT NULL REFERENCES actors(id),
               session_id TEXT REFERENCES sessions(id),
               data TEXT NOT NULL
             );
             INSERT INTO records_7 SELECT * FROM records;
             DROP TABLE records;
             ALTER TABLE records_7 RENAME TO records;
             CREATE INDEX records_session ON records(session_id, sequence);
             CREATE INDEX records_actor_kind ON records(actor_id, kind);
             CREATE UNIQUE INDEX records_spawned_session ON records(session_id)
               WHERE kind = 'spawned';
             ALTER TABLE messages DROP COLUMN queued_session_id;
             ALTER TABLE messages DROP COLUMN queued_at;
             ALTER TABLE messages DROP COLUMN label;
             ALTER TABLE actors DROP COLUMN grants;
             ALTER TABLE actors DROP COLUMN accept_from;
             ALTER TABLE sessions DROP COLUMN telemetry_bytes;
             PRAGMA user_version = 7;",
        )
        .unwrap();

    let store = Store::open(&path).unwrap();
    let after = records(&store);
    assert_eq!(
        after
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>()
    );
    // An Agent has no Schedule, so it has no tick to miss.
    assert!(
        !store
            .record_missed_tick("default", "agent", "2026-09-16T00:01:00.000Z", "it ended")
            .unwrap()
    );
    store
        .create_actor(
            "default",
            "job",
            feld_core::model::ActorKind::Job,
            &feld_core::model::LaunchSpec {
                runtime: None,
                runtime_settings: None,
                trust_workdir: None,
                command: Some(vec!["true".to_owned()]),
                workdir: Some("/tmp".to_owned()),
                env: None,
                provider: Some("tmux".to_owned()),
                node: Some("local".to_owned()),
                provider_options: None,
            },
            Some(&feld_daemon::store::ScheduleDeclaration {
                cron: "* * * * *".to_owned(),
                timezone: "UTC".to_owned(),
            }),
            None,
        )
        .unwrap();
    assert!(
        store
            .record_missed_tick("default", "job", "2026-09-16T00:01:00.000Z", "it ended")
            .unwrap()
    );
    let missed = records(&store).pop().unwrap();
    assert_eq!(missed.kind, feld_core::model::RecordKind::ScheduleMissed);
    assert!(missed.sequence > before.last().unwrap().sequence);
    // The same tick is recorded once.
    assert!(
        !store
            .record_missed_tick("default", "job", "2026-09-16T00:01:00.000Z", "it ended")
            .unwrap()
    );
}

#[test]
fn a_store_of_schema_9_takes_since_from_the_observation_records() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    let attached = store
        .admit_attach(
            "default",
            "agent",
            &attached_placement(102),
            attached_execution(),
            true,
            true,
        )
        .unwrap();
    let session_id = attached.pending.session.id.clone();
    for at in ["2026-09-16T00:00:05Z", "2026-09-16T00:00:09Z"] {
        store
            .record_observation(
                &session_id,
                &observation(ObservationKind::Lost, at, None),
                None,
            )
            .unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE sessions DROP COLUMN observation_since;
             ALTER TABLE messages DROP COLUMN queued_session_id;
             ALTER TABLE messages DROP COLUMN queued_at;
             ALTER TABLE messages DROP COLUMN label;
             ALTER TABLE actors DROP COLUMN grants;
             ALTER TABLE actors DROP COLUMN accept_from;
             ALTER TABLE sessions DROP COLUMN telemetry_bytes;
             PRAGMA user_version = 9;",
        )
        .unwrap();

    let store = Store::open(&path).unwrap();
    let observation = store
        .get_session(&session_id)
        .unwrap()
        .unwrap()
        .observation
        .unwrap();
    assert_eq!(observation.since.as_deref(), Some("2026-09-16T00:00:05Z"));
    assert_eq!(observation.at, "2026-09-16T00:00:09Z");
}

#[test]
fn a_store_of_another_schema_version_is_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("PRAGMA user_version = 99;")
        .unwrap();
    let Err(error) = Store::open(&path) else {
        panic!("a store of schema version 99 was opened");
    };
    let error = format!("{error:#}");
    assert!(
        error.contains("store schema version 99 is not supported"),
        "{error}"
    );
}

#[test]
fn range_telemetries_read_only_the_sessions_and_telemetry_of_their_range() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let instant = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let spawn = |actor: &str| {
        let launch = launch("/tmp/telemetry-range");
        store
            .create_actor("default", actor, ActorKind::Job, &launch, None, None)
            .unwrap();
        store
            .admit_start(
                "default",
                actor,
                &launch,
                &execution(),
                &placement("/tmp/telemetry-range"),
                RequestIdentity {
                    requester: &Requester::Admin,
                    session_id: None,
                },
            )
            .unwrap()
    };
    let telemetry = |request: &str, token: &str| {
        store
            .record_telemetry(
                request,
                &feld_core::telemetry::telemetry_token(token),
                "metrics",
                "application/x-protobuf",
                b"",
            )
            .unwrap();
    };
    let before_sessions = Utc::now() - Duration::seconds(1);
    let ended = spawn("ended");
    let current = spawn("current");
    telemetry("before-range", &current.token);
    // Received well before the range, beyond any clock difference of the measuring Node.
    rusqlite::Connection::open(temporary.path().join("feld.db"))
        .unwrap()
        .execute(
            "UPDATE telemetries SET received_at = ?1 WHERE request_id = 'before-range'",
            [instant(Utc::now() - Duration::hours(1))],
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    store
        .record_observation(
            &ended.session.id,
            &observation(ObservationKind::Gone, &instant(Utc::now()), None),
            None,
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let start = instant(Utc::now());
    telemetry("in-range", &current.token);

    let ids = |sessions: Vec<(
        feld_core::model::Session,
        Vec<feld_daemon::store::Telemetry>,
    )>| {
        sessions
            .into_iter()
            .map(|(session, telemetry)| {
                (
                    session.id,
                    telemetry
                        .into_iter()
                        .map(|telemetry| telemetry.request_id)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };
    // The latest Telemetries from before the range come along for the readings they hold.
    let live = ids(store
        .range_telemetries(Some(&start), None, |_| |_: &[_]| true)
        .unwrap());
    assert_eq!(
        live,
        [(
            current.session.id.clone(),
            vec!["before-range".to_owned(), "in-range".to_owned()]
        )]
    );
    let later = instant(Utc::now() + Duration::hours(1));
    let past = ids(store
        .range_telemetries(Some(&start), Some(&later), |_| |_: &[_]| true)
        .unwrap());
    assert_eq!(past, live);
    let earlier = store
        .range_telemetries(
            Some(&instant(before_sessions)),
            Some(&instant(before_sessions + Duration::milliseconds(1))),
            |_| |_: &[_]| true,
        )
        .unwrap();
    assert!(earlier.is_empty());
    let whole = ids(store
        .range_telemetries(None, None, |_| |_: &[_]| true)
        .unwrap());
    assert_eq!(
        whole,
        [
            (ended.session.id.clone(), Vec::new()),
            (
                current.session.id.clone(),
                vec!["before-range".to_owned(), "in-range".to_owned()]
            ),
        ]
    );
}

/// A store with one running Session of a Job.
fn store_with_telemetry_session(
    path: &std::path::Path,
) -> (Store, feld_daemon::store::PendingSession) {
    let store = Store::open(path).unwrap();
    let launch = launch("/tmp/telemetry-limit");
    store
        .create_actor("default", "job", ActorKind::Job, &launch, None, None)
        .unwrap();
    let started = store
        .admit_start(
            "default",
            "job",
            &launch,
            &execution(),
            &placement("/tmp/telemetry-limit"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    (store, started)
}

fn stored_request_ids(store: &Store, session_id: &str) -> Vec<String> {
    store
        .telemetries(session_id)
        .unwrap()
        .into_iter()
        .map(|telemetry| telemetry.request_id)
        .collect()
}

fn telemetry_bytes(path: &std::path::Path, session_id: &str) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT telemetry_bytes FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_session_keeps_its_newest_telemetry_up_to_the_limit() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let (store, started) = store_with_telemetry_session(&path);
    let third = feld_daemon::store::SESSION_TELEMETRY_LIMIT / 3
        - feld_daemon::store::TELEMETRY_ROW_OVERHEAD;
    for request in ["first", "second", "third", "fourth"] {
        let admission = store
            .record_telemetry(
                request,
                &feld_core::telemetry::telemetry_token(&started.token),
                "metrics",
                "application/x-protobuf",
                &vec![0; usize::try_from(third).unwrap()],
            )
            .unwrap();
        assert!(matches!(
            admission,
            feld_daemon::store::TelemetryAdmission::Stored { .. }
        ));
    }

    assert_eq!(
        stored_request_ids(&store, &started.session.id),
        ["second", "third", "fourth"]
    );
    assert_eq!(
        telemetry_bytes(&path, &started.session.id),
        3 * (third + feld_daemon::store::TELEMETRY_ROW_OVERHEAD)
    );
}

#[test]
fn a_session_keeps_empty_telemetry_up_to_the_limit_by_its_row_overhead() {
    let temporary = tempfile::tempdir().unwrap();
    let (store, started) = store_with_telemetry_session(&temporary.path().join("feld.db"));
    let rows =
        feld_daemon::store::SESSION_TELEMETRY_LIMIT / feld_daemon::store::TELEMETRY_ROW_OVERHEAD;
    for index in 0..rows + 3 {
        store
            .record_telemetry(
                &format!("empty-{index:06}"),
                &feld_core::telemetry::telemetry_token(&started.token),
                "metrics",
                "application/x-protobuf",
                b"",
            )
            .unwrap();
    }

    let kept = stored_request_ids(&store, &started.session.id);
    assert_eq!(kept.len(), usize::try_from(rows).unwrap());
    assert_eq!(kept.first().unwrap(), "empty-000003");
}

#[test]
fn telemetry_retention_brings_a_migrated_session_within_the_limit() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let (store, started) = store_with_telemetry_session(&path);
    drop(store);
    // A store of schema 14 kept every Telemetry, here 65 of 1 MiB.
    let connection = rusqlite::Connection::open(&path).unwrap();
    for index in 0..65 {
        connection
            .execute(
                "INSERT INTO telemetries
                 (request_id, session_id, signal, content_type, body, digest, received_at)
                 VALUES (?1, ?2, 'metrics', 'application/x-protobuf', zeroblob(1048576), '', ?3)",
                rusqlite::params![
                    format!("old-{index:02}"),
                    started.session.id,
                    format!("2026-10-01T00:00:{index:02}.000Z")
                ],
            )
            .unwrap();
    }
    connection
        .execute_batch(
            "ALTER TABLE sessions DROP COLUMN telemetry_bytes;
             PRAGMA user_version = 14;",
        )
        .unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    assert_eq!(stored_request_ids(&store, &started.session.id).len(), 65);
    let cutoff = (Utc::now() - Duration::days(30)).to_rfc3339_opts(SecondsFormat::Millis, true);
    assert_eq!(store.prune_telemetries(&cutoff).unwrap(), 2);
    let kept = stored_request_ids(&store, &started.session.id);
    assert_eq!(kept.len(), 63);
    assert_eq!(kept.first().unwrap(), "old-02");
    assert_eq!(
        telemetry_bytes(&path, &started.session.id),
        63 * (1_048_576 + feld_daemon::store::TELEMETRY_ROW_OVERHEAD)
    );
}

#[test]
fn telemetry_retention_removes_only_sessions_closed_before_the_cutoff() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let (store, started) = store_with_telemetry_session(&path);
    store
        .record_telemetry(
            "kept",
            &feld_core::telemetry::telemetry_token(&started.token),
            "metrics",
            "application/x-protobuf",
            b"",
        )
        .unwrap();
    let instant = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let cutoff = instant(Utc::now() - Duration::days(30));

    // A running Session keeps its Telemetry.
    assert_eq!(store.prune_telemetries(&cutoff).unwrap(), 0);
    store
        .record_observation(
            &started.session.id,
            &observation(ObservationKind::Gone, &instant(Utc::now()), None),
            None,
        )
        .unwrap();
    // A Session closed within the retention keeps it too.
    assert_eq!(store.prune_telemetries(&cutoff).unwrap(), 0);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE sessions SET end = ?1 WHERE id = ?2",
            rusqlite::params![instant(Utc::now() - Duration::days(31)), started.session.id],
        )
        .unwrap();
    assert_eq!(store.prune_telemetries(&cutoff).unwrap(), 1);
    assert!(stored_request_ids(&store, &started.session.id).is_empty());
    assert_eq!(telemetry_bytes(&path, &started.session.id), 0);
}

#[test]
fn session_telemetries_read_the_range_and_back_until_the_readings_before_it() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    let launch = launch("/tmp/telemetry-session-range");
    store
        .create_actor("default", "job", ActorKind::Job, &launch, None, None)
        .unwrap();
    let started = store
        .admit_start(
            "default",
            "job",
            &launch,
            &execution(),
            &placement("/tmp/telemetry-session-range"),
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let instant = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let start = Utc::now() - Duration::hours(2);
    let end = Utc::now() - Duration::hours(1);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let received = |request: &str, at: DateTime<Utc>| {
        store
            .record_telemetry(
                request,
                &feld_core::telemetry::telemetry_token(&started.token),
                "metrics",
                "application/x-protobuf",
                request.as_bytes(),
            )
            .unwrap();
        connection
            .execute(
                "UPDATE telemetries SET received_at = ?1 WHERE request_id = ?2",
                rusqlite::params![instant(at), request],
            )
            .unwrap();
    };
    // Two at a time share a received time, so a page can end between them.
    for index in 0..40 {
        received(
            &format!("before-{index:02}"),
            start - Duration::hours(1) + Duration::seconds((index + 1) / 2),
        );
    }
    // Within the clock allowance before the range, in it, and after it.
    received("early", start - Duration::minutes(1));
    received("in-range", start + Duration::minutes(30));
    received("after", end + Duration::hours(1));
    let request_ids = |telemetries: Vec<feld_daemon::store::Telemetry>| {
        telemetries
            .into_iter()
            .map(|telemetry| telemetry.request_id)
            .collect::<Vec<_>>()
    };
    let read = |holds: &mut dyn FnMut(&[feld_daemon::store::Telemetry]) -> bool| {
        request_ids(
            store
                .session_telemetries(&started.session.id, Some(&instant(start)), holds)
                .unwrap(),
        )
    };
    let expected = |from: usize| {
        let mut expected = (from..40)
            .map(|index| format!("before-{index:02}"))
            .collect::<Vec<_>>();
        expected.extend(["early", "in-range", "after"].map(str::to_owned));
        expected
    };

    // The latest page before the range holds the readings.
    assert_eq!(read(&mut |_| true), expected(24));
    // Pages go back until the pages so far hold the readings, each found in its own page.
    let mut found = [false; 2];
    let mut holds = |page: &[feld_daemon::store::Telemetry]| {
        for (index, wanted) in ["before-30", "before-10"].into_iter().enumerate() {
            found[index] |= page.iter().any(|telemetry| telemetry.request_id == wanted);
        }
        found == [true; 2]
    };
    assert_eq!(read(&mut holds), expected(8));
    // Or until none are left.
    assert_eq!(read(&mut |_| false), expected(0));

    let whole = request_ids(
        store
            .session_telemetries(&started.session.id, None, |_| false)
            .unwrap(),
    );
    assert_eq!(
        whole,
        request_ids(store.telemetries(&started.session.id).unwrap())
    );
    assert_eq!(whole.len(), 43);
}

#[test]
fn a_closed_store_leaves_no_wal_and_refuses_what_comes_after() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    let wal = temporary.path().join("feld.db-wal");
    let shm = temporary.path().join("feld.db-shm");
    assert!(wal.exists() && shm.exists());

    store.close().unwrap();

    assert!(!wal.exists() && !shm.exists());
    assert!(store.counts().is_err());
    assert!(store.create_workspace("after-close").is_err());
    store.close().unwrap();
}

/// A closed store reads its open Sessions without the daemon, and a missing store has
/// none.
#[test]
fn the_open_sessions_of_a_closed_store_read_without_the_daemon() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    assert!(
        feld_daemon::store::schema::read_open_sessions(&path)
            .unwrap()
            .is_empty()
    );
    let store = Store::open(&path).unwrap();
    let launch = launch("/tmp/open-sessions");
    let admit = |actor: &str| {
        store
            .create_actor("default", actor, ActorKind::Job, &launch, None, None)
            .unwrap();
        store
            .admit_start(
                "default",
                actor,
                &launch,
                &execution(),
                &placement("/tmp/open-sessions"),
                RequestIdentity {
                    requester: &Requester::Admin,
                    session_id: None,
                },
            )
            .unwrap()
            .session
    };
    let open = admit("open");
    let ended = admit("ended");
    store
        .record_launch_failure(
            &ended.id,
            &observation(ObservationKind::Gone, "2026-09-16T00:00:01Z", None),
        )
        .unwrap();
    store.close().unwrap();

    let read = feld_daemon::store::schema::read_open_sessions(&path).unwrap();
    let read: Vec<_> = read
        .iter()
        .map(|session| {
            (
                session.id.as_str(),
                session.workspace.as_str(),
                session.actor.as_str(),
                session.ownership,
                session.execution.as_str(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [(
            open.id.as_str(),
            "default",
            "open",
            Ownership::Owned,
            "program"
        )]
    );
}

/// A store left by a daemon that ended without closing it, its schema and an Actor still
/// in the write-ahead log, fails to read rather than reading as empty.
#[test]
fn the_open_sessions_of_a_store_with_writes_in_its_log_do_not_read() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    let launch = launch("/tmp/open-sessions");
    store
        .create_actor("default", "open", ActorKind::Job, &launch, None, None)
        .unwrap();
    // The files as a daemon that ended here leaves them.
    let left = temporary.path().join("left");
    std::fs::create_dir(&left).unwrap();
    for name in ["feld.db", "feld.db-wal"] {
        std::fs::copy(temporary.path().join(name), left.join(name)).unwrap();
    }
    let left = left.join("feld.db");
    assert_eq!(
        feld_daemon::store::schema::read_version(&left).unwrap(),
        Some(0)
    );

    let error = feld_daemon::store::schema::read_open_sessions(&left)
        .err()
        .expect("a store with writes in its log does not read");
    assert!(
        error
            .to_string()
            .contains("holds writes the daemon has not read back"),
        "{error:#}"
    );
}

/// A Session writes no Actor that trusts its workdir, which the write checks in its own
/// transaction; the Admin writes it.
#[test]
fn a_session_writes_no_actor_that_trusts_its_workdir() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let trusted = LaunchSpec {
        runtime: Some("codex".to_owned()),
        trust_workdir: Some(true),
        command: None,
        ..launch("/tmp/trusted")
    };
    let agents = Writer::Session {
        agents: true,
        jobs: false,
    };
    store
        .create_actor("default", "lead", ActorKind::Agent, &trusted, None, None)
        .unwrap();
    let refused = |result: Result<_, ActorWriteError>| {
        assert!(
            matches!(result, Err(ActorWriteError::Trusted(ref name)) if name == "lead"),
            "{result:?}"
        );
    };
    refused(store.create_actor_as(
        "default",
        "lead",
        ActorKind::Agent,
        &launch("/tmp/rogue"),
        None,
        None,
        None,
        agents,
    ));
    refused(store.update_actor_as("default", "lead", ActorPatch::default(), "UTC", agents));
    refused(store.delete_actor_as("default", "lead", agents));
    store
        .update_actor_as(
            "default",
            "lead",
            ActorPatch::default(),
            "UTC",
            Writer::Admin,
        )
        .unwrap();
}

/// A Session writes only an Actor without grants of a kind its grants manage, which the
/// write checks in its own transaction; the Admin writes any.
#[test]
fn a_session_writes_only_managed_actors_without_grants() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let launch = launch("/tmp/granted");
    let grants = [Grant::Read].into_iter().collect();
    let jobs = Writer::Session {
        agents: false,
        jobs: true,
    };
    let agents = Writer::Session {
        agents: true,
        jobs: false,
    };
    store
        .create_actor(
            "default",
            "lead",
            ActorKind::Job,
            &launch,
            None,
            Some(&grants),
        )
        .unwrap();
    store
        .create_actor("default", "plain", ActorKind::Job, &launch, None, None)
        .unwrap();
    let granted = |result: Result<_, ActorWriteError>| {
        assert!(
            matches!(result, Err(ActorWriteError::Granted(ref name)) if name == "lead"),
            "{result:?}"
        );
    };
    granted(store.create_actor_as(
        "default",
        "lead",
        ActorKind::Job,
        &launch,
        None,
        None,
        None,
        jobs,
    ));
    granted(store.update_actor_as("default", "lead", ActorPatch::default(), "UTC", jobs));
    granted(store.delete_actor_as("default", "lead", jobs));

    // A Job, though it was an Agent when the request was judged, is not an Agent's to write.
    let unmanaged = |result: Result<_, ActorWriteError>| {
        assert!(
            matches!(
                result,
                Err(ActorWriteError::Unmanaged {
                    kind: ActorKind::Job,
                    ..
                })
            ),
            "{result:?}"
        );
    };
    unmanaged(store.delete_actor_as("default", "plain", agents));
    unmanaged(store.update_actor_as("default", "plain", ActorPatch::default(), "UTC", agents));
    unmanaged(store.create_actor_as(
        "default",
        "fresh",
        ActorKind::Job,
        &launch,
        None,
        None,
        None,
        agents,
    ));

    let schedule = feld_core::model::SchedulePatch {
        cron: Some("0 9 * * *".to_owned()),
        timezone: None,
        enabled: None,
    };
    let schedule_patch = ActorPatch {
        schedule: Some(&schedule),
        ..ActorPatch::default()
    };
    store
        .update_actor_as("default", "plain", schedule_patch, "UTC", jobs)
        .unwrap();
    store.delete_actor_as("default", "plain", jobs).unwrap();
    store
        .update_actor_as("default", "lead", schedule_patch, "UTC", Writer::Admin)
        .unwrap();
}
