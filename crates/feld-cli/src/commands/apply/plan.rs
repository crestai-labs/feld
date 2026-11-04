//! The difference between what a declaration file declares and the Actors of its Workspace.

use std::collections::BTreeMap;

use feld_core::model::{Actor, ActorKind};
use serde::Serialize;
use serde_json::{Value, json};

use super::{Declaration, DeclaredActor};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// A new Actor.
    Create,
    /// A live Actor whose declaration differs.
    Replace,
    /// An Actor an earlier apply of the file declared, which the file no longer has.
    Delete,
}

impl Action {
    pub fn mark(self) -> char {
        match self {
            Self::Create => '+',
            Self::Replace => '~',
            Self::Delete => '-',
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub action: Action,
    pub actor: String,
    pub kind: ActorKind,
    /// The fields that differ, by their dotted path. A delete names none.
    pub fields: Vec<FieldChange>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// The changes that make `current`, the live Actors of the Workspace, match the declaration:
/// the declared Actors in the order of the file, then the deletes by name. An Actor of
/// another kind under a declared name is refused, since an Actor keeps its kind.
pub fn plan(declaration: &Declaration, current: &[Actor]) -> Result<Vec<Change>, String> {
    let live: BTreeMap<&str, &Actor> = current
        .iter()
        .filter(|actor| actor.deleted_at.is_none())
        .map(|actor| (actor.name.as_str(), actor))
        .collect();
    let mut changes = Vec::new();
    for declared in &declaration.actors {
        let after = declared_fields(declared, &declaration.name);
        let Some(actor) = live.get(declared.name.as_str()) else {
            changes.push(Change {
                action: Action::Create,
                actor: declared.name.clone(),
                kind: declared.kind,
                fields: after
                    .into_iter()
                    .map(|(field, value)| FieldChange {
                        field,
                        before: None,
                        after: Some(value),
                    })
                    .collect(),
            });
            continue;
        };
        if actor.kind != declared.kind {
            return Err(format!(
                "Actor '{}' is a {} and the file declares a {}; an Actor keeps its kind, so delete it with `feld actor delete {}` first",
                actor.name, actor.kind, declared.kind, actor.name
            ));
        }
        let fields = differences(&current_fields(actor), &after);
        if !fields.is_empty() {
            changes.push(Change {
                action: Action::Replace,
                actor: declared.name.clone(),
                kind: declared.kind,
                fields,
            });
        }
    }
    let mut removed: Vec<&Actor> = live
        .values()
        .copied()
        .filter(|actor| actor.declared_in.as_deref() == Some(declaration.name.as_str()))
        .filter(|actor| {
            !declaration
                .actors
                .iter()
                .any(|declared| declared.name == actor.name)
        })
        .collect();
    removed.sort_by(|left, right| left.name.cmp(&right.name));
    changes.extend(removed.into_iter().map(|actor| Change {
        action: Action::Delete,
        actor: actor.name.clone(),
        kind: actor.kind,
        fields: Vec::new(),
    }));
    Ok(changes)
}

/// The fields that differ. A field the file leaves to the daemon is not compared: an option
/// under `providerOptions` the file does not name, which the Execution Provider fills, and the
/// `timezone` and `enabled` of a declared Schedule the file leaves out.
fn differences(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
) -> Vec<FieldChange> {
    let declares_schedule = after.contains_key("schedule.cron");
    let left_to_daemon = |field: &str| {
        field == "providerOptions"
            || field.starts_with("providerOptions.")
            || (declares_schedule && matches!(field, "schedule.timezone" | "schedule.enabled"))
    };
    let mut fields: Vec<&String> = before.keys().chain(after.keys()).collect();
    fields.sort();
    fields.dedup();
    fields
        .into_iter()
        .filter_map(|field| {
            let (old, new) = (before.get(field), after.get(field));
            if old == new || (new.is_none() && left_to_daemon(field)) {
                return None;
            }
            Some(FieldChange {
                field: field.clone(),
                before: old.cloned(),
                after: new.cloned(),
            })
        })
        .collect()
}

fn declared_fields(declared: &DeclaredActor, file: &str) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    flatten("", &json!(declared.launch), &mut fields);
    // Empty `providerOptions` leave every option to the Execution Provider, as omitted ones do.
    if fields
        .get("providerOptions")
        .is_some_and(|options| options.as_object().is_some_and(serde_json::Map::is_empty))
    {
        fields.remove("providerOptions");
    }
    if let Some(schedule) = &declared.schedule {
        fields.insert("schedule.cron".to_owned(), json!(schedule.cron));
        if let Some(timezone) = &schedule.timezone {
            fields.insert("schedule.timezone".to_owned(), json!(timezone));
        }
        if let Some(enabled) = schedule.enabled {
            fields.insert("schedule.enabled".to_owned(), json!(enabled));
        }
    }
    fields.insert("grants".to_owned(), json!(declared.grants));
    fields.insert("acceptFrom".to_owned(), json!(declared.accept_from));
    fields.insert("declaredIn".to_owned(), json!(file));
    fields
}

fn current_fields(actor: &Actor) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    if let Some(launch) = &actor.launch_spec {
        flatten("", &json!(launch), &mut fields);
    }
    if let Some(schedule) = &actor.schedule {
        fields.insert("schedule.cron".to_owned(), json!(schedule.cron));
        fields.insert("schedule.timezone".to_owned(), json!(schedule.timezone));
        fields.insert("schedule.enabled".to_owned(), json!(schedule.enabled));
    }
    fields.insert("grants".to_owned(), json!(actor.grants));
    fields.insert("acceptFrom".to_owned(), json!(actor.accept_from));
    fields.insert("declaredIn".to_owned(), json!(actor.declared_in));
    fields
}

/// Each leaf of `value` by its dotted path. An array and an empty object are leaves.
fn flatten(prefix: &str, value: &Value, fields: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(object) if !object.is_empty() || prefix.is_empty() => {
            for (key, value) in object {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&path, value, fields);
            }
        }
        _ => {
            fields.insert(prefix.to_owned(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use feld_core::model::{Grant, LaunchSpec, Schedule, ScheduleDeclaration};

    use super::*;
    use crate::commands::apply::DeclaredSchedule;

    fn launch(command: &str) -> LaunchSpec {
        LaunchSpec {
            command: Some(vec![command.to_owned()]),
            workdir: Some("/repo".to_owned()),
            env: Some(BTreeMap::from([("EPIC".to_owned(), "96".to_owned())])),
            provider: Some("tmux".to_owned()),
            node: Some("local".to_owned()),
            ..LaunchSpec::default()
        }
    }

    fn declared(name: &str) -> DeclaredActor {
        DeclaredActor {
            name: name.to_owned(),
            kind: ActorKind::Job,
            launch: launch("run"),
            schedule: Some(DeclaredSchedule {
                cron: "*/15 * * * *".to_owned(),
                timezone: None,
                enabled: None,
            }),
            grants: BTreeSet::from([Grant::Read]),
            accept_from: None,
        }
    }

    fn actor(name: &str, declared_in: Option<&str>) -> Actor {
        let mut actor: Actor = serde_json::from_value(json!({
            "id": format!("actor_{name}"),
            "name": name,
            "workspace": "default",
            "kind": "job",
            "launchSpec": null,
            "schedule": null,
            "grants": ["read"],
            "acceptFrom": null,
            "declaredIn": declared_in,
            "createdAt": "2026-11-04T00:00:00Z",
            "updatedAt": "2026-11-04T00:00:00Z",
            "deletedAt": null,
            "currentSession": null,
            "latestSession": null,
            "sessions": [],
            "attention": false,
            "waitingCount": 0,
            "unreadMail": false,
            "skipMark": false,
            "unlinkedMark": false,
        }))
        .expect("actor");
        let mut launch = launch("run");
        launch.provider_options = Some(json!({ "socket": "default", "unit": "window" }));
        actor.launch_spec = Some(launch);
        let mut schedule = Schedule::declare(
            &ScheduleDeclaration {
                cron: "*/15 * * * *".to_owned(),
                timezone: "Asia/Tokyo".to_owned(),
            },
            None,
            "2026-11-04T00:00:00Z",
        );
        schedule.enabled = false;
        actor.schedule = Some(schedule);
        actor
    }

    fn declaration(actors: Vec<DeclaredActor>) -> Declaration {
        Declaration {
            name: "harness".to_owned(),
            workspace: None,
            actors,
        }
    }

    #[test]
    fn an_applied_actor_matches_without_the_fields_the_daemon_fills() {
        let changes = plan(
            &declaration(vec![declared("dispatcher")]),
            &[actor("dispatcher", Some("harness"))],
        )
        .expect("plan");
        assert!(changes.is_empty(), "{changes:?}");
    }

    #[test]
    fn empty_provider_options_leave_the_options_to_the_provider() {
        let mut empty = declared("dispatcher");
        empty.launch.provider_options = Some(json!({}));
        let changes = plan(
            &declaration(vec![empty]),
            &[actor("dispatcher", Some("harness"))],
        )
        .expect("plan");
        assert!(changes.is_empty(), "{changes:?}");
    }

    #[test]
    fn creates_replaces_and_deletes_only_the_actors_of_the_file() {
        let mut changed = declared("dispatcher");
        changed
            .launch
            .env
            .as_mut()
            .expect("env")
            .insert("EPIC".to_owned(), "97".to_owned());
        changed.schedule.as_mut().expect("schedule").enabled = Some(true);
        let changes = plan(
            &declaration(vec![declared("planner"), changed]),
            &[
                actor("dispatcher", Some("harness")),
                actor("nightly", Some("harness")),
                actor("other", Some("elsewhere")),
                actor("manual", None),
            ],
        )
        .expect("plan");
        let summary: Vec<(Action, &str)> = changes
            .iter()
            .map(|change| (change.action, change.actor.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (Action::Create, "planner"),
                (Action::Replace, "dispatcher"),
                (Action::Delete, "nightly"),
            ]
        );
        let fields: Vec<&str> = changes[1]
            .fields
            .iter()
            .map(|field| field.field.as_str())
            .collect();
        assert_eq!(fields, ["env.EPIC", "schedule.enabled"]);
    }

    #[test]
    fn a_hand_made_actor_is_taken_over_and_another_kind_is_refused() {
        let changes = plan(
            &declaration(vec![declared("dispatcher")]),
            &[actor("dispatcher", None)],
        )
        .expect("plan");
        assert_eq!(changes[0].fields[0].field, "declaredIn");
        let mut agent = declared("dispatcher");
        agent.kind = ActorKind::Agent;
        assert!(plan(&declaration(vec![agent]), &[actor("dispatcher", None)]).is_err());
    }

    #[test]
    fn a_removed_schedule_and_env_show_as_removed() {
        let mut bare = declared("dispatcher");
        bare.schedule = None;
        bare.launch.env = None;
        let changes = plan(
            &declaration(vec![bare]),
            &[actor("dispatcher", Some("harness"))],
        )
        .expect("plan");
        let fields: Vec<(&str, bool)> = changes[0]
            .fields
            .iter()
            .map(|field| (field.field.as_str(), field.after.is_none()))
            .collect();
        assert_eq!(
            fields,
            [
                ("env.EPIC", true),
                ("schedule.cron", true),
                ("schedule.enabled", true),
                ("schedule.timezone", true),
            ]
        );
    }
}
