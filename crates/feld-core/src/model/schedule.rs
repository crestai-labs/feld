use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::cron::{next_run_after, validate_cron, validate_timezone};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub cron: String,
    pub timezone: String,
    pub enabled: bool,
    pub created_at: String,
    pub paused_at: Option<String>,
    pub enabled_at: Option<String>,
    pub next_run: Option<String>,
    /// How many ticks admission refused, from the `schedule.skipped` records of the Actor.
    #[serde(default)]
    pub skipped_ticks: u64,
    /// The planned time of a tick that should have fired by now and did not.
    #[serde(default)]
    pub overdue_tick: Option<String>,
    #[serde(skip)]
    pub revision: u64,
}

/// The `cron` and `timezone` an Actor declaration gives its Schedule.
#[derive(Clone, Debug)]
pub struct ScheduleDeclaration {
    pub cron: String,
    pub timezone: String,
}

/// The Schedule fields an Actor update changes; each `None` keeps the field.
#[derive(Clone, Debug, Default)]
pub struct SchedulePatch {
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub enabled: Option<bool>,
}

impl Schedule {
    pub(super) fn new(cron: String, timezone: String, enabled: bool, at: &str) -> Self {
        Self {
            cron,
            timezone,
            enabled,
            created_at: at.to_owned(),
            paused_at: (!enabled).then(|| at.to_owned()),
            enabled_at: enabled.then(|| at.to_owned()),
            next_run: None,
            skipped_ticks: 0,
            overdue_tick: None,
            revision: 0,
        }
    }

    /// The Schedule an Actor declaration gives: the declared `cron` and `timezone` over the
    /// `existing` Schedule, which keeps `enabled`, or a new enabled one.
    pub fn declare(declaration: &ScheduleDeclaration, existing: Option<Self>, at: &str) -> Self {
        let cron = declaration.cron.clone();
        let timezone = declaration.timezone.clone();
        match existing {
            Some(existing) => Self {
                cron,
                timezone,
                ..existing
            },
            None => Self::new(cron, timezone, true, at),
        }
    }

    /// The Schedule a patch creates on an Actor without one, which needs its `cron`.
    pub fn from_patch(patch: &SchedulePatch, at: &str, default_timezone: &str) -> Option<Self> {
        Some(Self::new(
            patch.cron.clone()?,
            patch
                .timezone
                .clone()
                .unwrap_or_else(|| default_timezone.to_owned()),
            patch.enabled.unwrap_or(true),
            at,
        ))
    }

    /// The Schedule after a patch at `at`: turning `enabled` off stamps `pausedAt`, and
    /// turning it on stamps `enabledAt`.
    pub fn apply_patch(self, patch: &SchedulePatch, at: &str) -> Self {
        let enabled = patch.enabled.unwrap_or(self.enabled);
        Self {
            cron: patch.cron.clone().unwrap_or(self.cron),
            timezone: patch.timezone.clone().unwrap_or(self.timezone),
            enabled,
            paused_at: if self.enabled && !enabled {
                Some(at.to_owned())
            } else {
                self.paused_at
            },
            enabled_at: if !self.enabled && enabled {
                Some(at.to_owned())
            } else {
                self.enabled_at
            },
            ..self
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_cron(&self.cron)?;
        validate_timezone(&self.timezone)
    }

    /// Whether this Schedule still fires the tick planned from `expected`: it is enabled and
    /// nothing changed since.
    pub fn fires_as(&self, expected: &Schedule) -> bool {
        self.enabled
            && self.cron == expected.cron
            && self.timezone == expected.timezone
            && self.created_at == expected.created_at
            && self.enabled_at == expected.enabled_at
            && self.revision == expected.revision
    }

    pub fn derive_next_run(&mut self) {
        self.next_run = if self.enabled {
            next_run_after(&self.cron, &self.timezone, Utc::now()).ok()
        } else {
            None
        };
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSkip {
    pub planned: String,
    pub current_session_id: String,
    pub at: String,
}
