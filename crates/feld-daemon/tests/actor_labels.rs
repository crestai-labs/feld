use chrono::{Duration, Utc};
use feld_core::model::{LabelSetter, Placement, Record, RecordKind};
use feld_daemon::store::{ActorWriteError, AttachedExecution, LabelWriter, RecordFilter, Store};
use serde_json::json;

fn placement(runtime_pid: u32) -> Placement {
    Placement {
        provider: "tmux".to_owned(),
        node: "local".to_owned(),
        workdir: "/tmp".to_owned(),
        placement: json!({
            "endpoint": {"kind": "path", "value": "/tmp/feld-labels.sock"},
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

/// An attached Agent `actor` and the `id` of its Session.
fn attach(store: &Store, actor: &str, runtime_pid: u32) -> String {
    store
        .admit_attach(
            "default",
            actor,
            &placement(runtime_pid),
            AttachedExecution {
                runtime: "claude-code",
                label: "Claude Code",
            },
            true,
            true,
        )
        .unwrap()
        .pending
        .session
        .id
}

fn label_records(store: &Store) -> Vec<Record> {
    store
        .list_records(&RecordFilter::default())
        .unwrap()
        .into_iter()
        .filter(|record| record.kind.as_str().starts_with("label."))
        .collect()
}

fn later(minutes: i64) -> String {
    feld_core::time::format(Utc::now() + Duration::minutes(minutes))
}

#[test]
fn a_label_is_set_replaced_and_cleared_with_its_records() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let session = attach(&store, "developer", 102);
    let until = later(30);

    let actor = store
        .set_label(
            "default",
            "developer",
            "waiting-upstream",
            Some(&until),
            Some("the monthly job holds the API"),
            LabelWriter::Session(&session),
        )
        .unwrap();
    assert_eq!(actor.labels.len(), 1);
    let label = &actor.labels[0];
    assert_eq!(label.key, "waiting-upstream");
    assert_eq!(label.until.as_deref(), Some(until.as_str()));
    assert_eq!(label.by, LabelSetter::Actor);

    let actor = store
        .set_label(
            "default",
            "developer",
            "waiting-upstream",
            None,
            None,
            LabelWriter::Admin,
        )
        .unwrap();
    assert_eq!(actor.labels.len(), 1);
    assert_eq!(actor.labels[0].until, None);
    assert_eq!(actor.labels[0].reason, None);
    assert_eq!(actor.labels[0].by, LabelSetter::Admin);

    let actor = store
        .clear_label(
            "default",
            "developer",
            "waiting-upstream",
            LabelWriter::Admin,
        )
        .unwrap();
    assert!(actor.labels.is_empty());
    // Clearing a key the Actor lacks changes nothing and records nothing.
    store
        .clear_label(
            "default",
            "developer",
            "waiting-upstream",
            LabelWriter::Admin,
        )
        .unwrap();

    let records = label_records(&store);
    let kinds: Vec<_> = records.iter().map(|record| record.kind).collect();
    assert_eq!(
        kinds,
        [
            RecordKind::LabelSet,
            RecordKind::LabelSet,
            RecordKind::LabelCleared
        ]
    );
    assert_eq!(records[0].session_id.as_deref(), Some(session.as_str()));
    assert_eq!(records[0].data["by"], "actor");
    assert_eq!(records[0].data["reason"], "the monthly job holds the API");
    assert_eq!(records[1].session_id, None);
    assert_eq!(
        records[2].data,
        json!({"key": "waiting-upstream", "by": "admin"})
    );
}

#[test]
fn a_session_labels_only_its_own_actor() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    let session = attach(&store, "developer", 102);
    attach(&store, "reviewer", 103);

    let refused = store.set_label(
        "default",
        "reviewer",
        "phase-review",
        None,
        None,
        LabelWriter::Session(&session),
    );
    assert!(matches!(refused, Err(ActorWriteError::OthersLabel(ref actor)) if actor == "reviewer"));
    let refused = store.clear_label(
        "default",
        "reviewer",
        "phase-review",
        LabelWriter::Session(&session),
    );
    assert!(matches!(refused, Err(ActorWriteError::OthersLabel(_))));
    assert!(matches!(
        store.set_label("default", "nobody", "x", None, None, LabelWriter::Admin),
        Err(ActorWriteError::ActorNotFound { .. })
    ));
    assert!(label_records(&store).is_empty());
}

#[test]
fn a_label_needs_a_key_of_the_grammar_and_an_until_after_now() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(&temporary.path().join("feld.db")).unwrap();
    attach(&store, "developer", 102);
    for (key, until) in [
        ("Waiting", None),
        ("-waiting", None),
        ("waiting", Some("tomorrow")),
        ("waiting", Some("2020-01-01T00:00:00Z")),
    ] {
        let refused = store.set_label("default", "developer", key, until, None, LabelWriter::Admin);
        assert!(
            matches!(refused, Err(ActorWriteError::InvalidLabel(_))),
            "{key} {until:?}"
        );
    }
    // An offset other than UTC names the same instant, kept in UTC.
    let actor = store
        .set_label(
            "default",
            "developer",
            "waiting",
            Some("2999-01-01T09:00:00+09:00"),
            None,
            LabelWriter::Admin,
        )
        .unwrap();
    assert_eq!(
        actor.labels[0].until.as_deref(),
        Some("2999-01-01T00:00:00.000Z")
    );
}

#[test]
fn a_label_past_its_until_is_hidden_then_removed_as_expired() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    attach(&store, "developer", 102);
    for key in ["waiting-upstream", "phase-review"] {
        let until = (key == "waiting-upstream").then(|| later(30));
        store
            .set_label(
                "default",
                "developer",
                key,
                until.as_deref(),
                None,
                LabelWriter::Admin,
            )
            .unwrap();
    }
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE actor_labels SET until = '2026-01-01T00:00:00.000Z' WHERE key = 'waiting-upstream'",
            [],
        )
        .unwrap();

    let keys = |store: &Store| -> Vec<String> {
        store
            .get_actor("default", "developer")
            .unwrap()
            .unwrap()
            .labels
            .into_iter()
            .map(|label| label.key)
            .collect()
    };
    assert_eq!(keys(&store), ["phase-review"]);
    assert!(
        !label_records(&store)
            .iter()
            .any(|record| record.kind == RecordKind::LabelExpired)
    );

    let mut events = store.subscribe_events();
    store.expire_labels().unwrap();
    store.expire_labels().unwrap();
    assert_eq!(keys(&store), ["phase-review"]);
    let expired: Vec<_> = label_records(&store)
        .into_iter()
        .filter(|record| record.kind == RecordKind::LabelExpired)
        .collect();
    assert_eq!(expired.len(), 1);
    assert_eq!(
        expired[0].data,
        json!({"key": "waiting-upstream", "until": "2026-01-01T00:00:00.000Z", "by": "admin"})
    );
    assert_eq!(events.try_recv().unwrap().id, "developer");
}

#[test]
fn a_store_of_schema_15_keeps_its_records_and_takes_labels() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("feld.db");
    let store = Store::open(&path).unwrap();
    attach(&store, "developer", 102);
    let before = store.list_records(&RecordFilter::default()).unwrap();
    assert!(!before.is_empty());
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "DROP TABLE actor_labels;
             ALTER TABLE actors DROP COLUMN declared_in;
             CREATE TABLE records_15 (
               sequence INTEGER PRIMARY KEY AUTOINCREMENT,
               at TEXT NOT NULL,
               kind TEXT NOT NULL CHECK (kind IN ('session.started', 'session.ended', 'spawned',
                 'message.accepted', 'message.sent', 'message.read', 'stop.requested',
                 'kill.requested', 'observation', 'schedule.skipped', 'schedule.missed')),
               actor_id TEXT NOT NULL REFERENCES actors(id),
               session_id TEXT REFERENCES sessions(id),
               data TEXT NOT NULL
             );
             INSERT INTO records_15 SELECT * FROM records;
             DROP TABLE records;
             ALTER TABLE records_15 RENAME TO records;
             CREATE INDEX records_session ON records(session_id, sequence);
             CREATE INDEX records_actor_kind ON records(actor_id, kind);
             CREATE UNIQUE INDEX records_spawned_session ON records(session_id)
               WHERE kind = 'spawned';
             PRAGMA user_version = 15;",
        )
        .unwrap();

    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.list_records(&RecordFilter::default()).unwrap(),
        before
    );
    let actor = store
        .set_label(
            "default",
            "developer",
            "phase-review",
            None,
            None,
            LabelWriter::Admin,
        )
        .unwrap();
    assert_eq!(actor.labels[0].key, "phase-review");
    assert_eq!(label_records(&store).len(), 1);
}
