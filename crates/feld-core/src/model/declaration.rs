//! An Actor declaration as its fields by dotted path, and the difference between two.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Actor, Grant, LaunchSpec};

/// What declaring an Actor did, or would do.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeclareAction {
    /// A new Actor.
    Create,
    /// A live Actor whose declaration differs.
    Replace,
    /// A live Actor whose declaration already matches.
    Unchanged,
}

/// One field of an Actor declaration as it changes, by its dotted path (`env.EPIC`,
/// `schedule.cron`, `grants`). `before` is None when the field was absent, `after` when it
/// is removed.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// The `cron`, `timezone` and `enabled` of a Schedule, which are what its declaration sets.
#[derive(Clone, Copy, Debug)]
pub struct ScheduleFields<'a> {
    pub cron: &'a str,
    pub timezone: &'a str,
    pub enabled: bool,
}

/// The declaration of an Actor, as the fields a declaration compares.
#[derive(Clone, Copy, Debug)]
pub struct DeclarationFields<'a> {
    pub launch: Option<&'a LaunchSpec>,
    pub schedule: Option<ScheduleFields<'a>>,
    pub grants: &'a BTreeSet<Grant>,
    pub accept_from: Option<&'a BTreeSet<String>>,
    pub declared_in: Option<&'a str>,
}

impl<'a> DeclarationFields<'a> {
    /// The declaration a live Actor holds.
    pub fn of(actor: &'a Actor) -> Self {
        Self {
            launch: actor.launch_spec.as_ref(),
            schedule: actor.schedule.as_ref().map(|schedule| ScheduleFields {
                cron: &schedule.cron,
                timezone: &schedule.timezone,
                enabled: schedule.enabled,
            }),
            grants: &actor.grants,
            accept_from: actor.accept_from.as_ref(),
            declared_in: actor.declared_in.as_deref(),
        }
    }

    /// Each field by its dotted path. An array and an empty object are leaves.
    pub fn flatten(&self) -> BTreeMap<String, Value> {
        let mut fields = BTreeMap::new();
        if let Some(launch) = self.launch {
            flatten("", &json!(launch), &mut fields);
        }
        if let Some(schedule) = &self.schedule {
            fields.insert("schedule.cron".to_owned(), json!(schedule.cron));
            fields.insert("schedule.timezone".to_owned(), json!(schedule.timezone));
            fields.insert("schedule.enabled".to_owned(), json!(schedule.enabled));
        }
        fields.insert("grants".to_owned(), json!(self.grants));
        fields.insert("acceptFrom".to_owned(), json!(self.accept_from));
        fields.insert("declaredIn".to_owned(), json!(self.declared_in));
        fields
    }

    /// The fields that differ from `self` to `after`, by their path.
    pub fn changes_to(&self, after: &DeclarationFields<'_>) -> Vec<FieldChange> {
        field_changes(&self.flatten(), &after.flatten())
    }
}

/// The fields that differ between two flattened declarations, by their path.
pub fn field_changes(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
) -> Vec<FieldChange> {
    let mut fields: Vec<&String> = before.keys().chain(after.keys()).collect();
    fields.sort();
    fields.dedup();
    fields
        .into_iter()
        .filter_map(|field| {
            let (old, new) = (before.get(field), after.get(field));
            (old != new).then(|| FieldChange {
                field: field.clone(),
                before: old.cloned(),
                after: new.cloned(),
            })
        })
        .collect()
}

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
    use super::*;

    fn launch(epic: &str) -> LaunchSpec {
        LaunchSpec {
            command: Some(vec!["run".to_owned()]),
            workdir: Some("/repo".to_owned()),
            env: Some(BTreeMap::from([("EPIC".to_owned(), epic.to_owned())])),
            provider: Some("tmux".to_owned()),
            node: Some("local".to_owned()),
            provider_options: Some(json!({ "socket": "default", "unit": "window" })),
            ..LaunchSpec::default()
        }
    }

    #[test]
    fn flattens_each_leaf_by_its_path() {
        let grants = BTreeSet::from([Grant::Read]);
        let fields = DeclarationFields {
            launch: Some(&launch("96")),
            schedule: Some(ScheduleFields {
                cron: "*/15 * * * *",
                timezone: "Asia/Tokyo",
                enabled: false,
            }),
            grants: &grants,
            accept_from: None,
            declared_in: Some("harness"),
        }
        .flatten();
        assert_eq!(fields["env.EPIC"], json!("96"));
        assert_eq!(fields["providerOptions.socket"], json!("default"));
        assert_eq!(fields["schedule.enabled"], json!(false));
        assert_eq!(fields["grants"], json!(["read"]));
        assert_eq!(fields["acceptFrom"], Value::Null);
        assert_eq!(fields["declaredIn"], json!("harness"));
        assert!(!fields.contains_key("runtime"), "{fields:?}");
    }

    #[test]
    fn changes_name_the_fields_that_differ_and_the_ones_removed() {
        let before_grants = BTreeSet::from([Grant::Read]);
        let after_grants = BTreeSet::new();
        let before = DeclarationFields {
            launch: Some(&launch("96")),
            schedule: Some(ScheduleFields {
                cron: "*/15 * * * *",
                timezone: "Asia/Tokyo",
                enabled: true,
            }),
            grants: &before_grants,
            accept_from: None,
            declared_in: None,
        };
        let changed = launch("97");
        let after = DeclarationFields {
            launch: Some(&changed),
            schedule: None,
            grants: &after_grants,
            accept_from: None,
            declared_in: Some("harness"),
        };
        let changes = before.changes_to(&after);
        let summary: Vec<(&str, Option<&Value>, Option<&Value>)> = changes
            .iter()
            .map(|change| {
                (
                    change.field.as_str(),
                    change.before.as_ref(),
                    change.after.as_ref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("declaredIn", Some(&Value::Null), Some(&json!("harness"))),
                ("env.EPIC", Some(&json!("96")), Some(&json!("97"))),
                ("grants", Some(&json!(["read"])), Some(&json!([]))),
                ("schedule.cron", Some(&json!("*/15 * * * *")), None),
                ("schedule.enabled", Some(&json!(true)), None),
                ("schedule.timezone", Some(&json!("Asia/Tokyo")), None),
            ]
        );
        assert!(before.changes_to(&before).is_empty());
    }
}
