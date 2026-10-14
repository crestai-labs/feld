use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Notify;

use crate::store::{ActorPatch, ActorWriteError, LaunchUpdate, Store, Writer};
use feld_core::id::valid_key;
use feld_core::model::{
    Actor, ActorKind, Grant, LaunchSpec, LaunchSpecPatch, ScheduleDeclaration, SchedulePatch,
    last_run_at_or_before, validate_cron, validate_timezone,
};
use feld_core::plugin::protocol::ResolveRequest;
use feld_core::plugin::{NODES, Plugins, Registry};
use feld_core::time;

#[derive(Debug, thiserror::Error)]
pub enum ActorError {
    /// The request itself is refused, in the words given.
    #[error("{0}")]
    Invalid(String),
    #[error("Workspace '{0}' was not found")]
    WorkspaceNotFound(String),
    #[error(transparent)]
    Write(#[from] ActorWriteError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// How long a tick may take to fire before the Actor shows it as overdue.
const OVERDUE_AFTER: chrono::Duration = chrono::Duration::seconds(60);

/// What a new Schedule declares: its `timezone` falls back to the daemon's.
pub struct ScheduleRequest {
    pub cron: String,
    pub timezone: Option<String>,
}

/// What a creation declares of an Actor beside its name.
pub struct ActorDeclaration<'a> {
    pub kind: ActorKind,
    pub launch: &'a LaunchSpec,
    pub schedule: Option<ScheduleRequest>,
    /// The grants the declaration gives, kept from a replaced declaration when None.
    pub grants: Option<&'a BTreeSet<Grant>>,
    /// The names of the Actors it accepts Messages from, kept from a replaced declaration
    /// when None.
    pub accept_from: Option<&'a BTreeSet<String>>,
    pub writer: Writer,
}

/// Actor declarations: their creation, update and deletion, with the Execution Provider's
/// `resolve` of the launchSpec and the wake-up of the Scheduler.
pub struct ActorService {
    store: Arc<Store>,
    registry: Arc<Registry>,
    /// The timezone a Schedule declared without one runs in.
    timezone: String,
    schedule_notify: Arc<Notify>,
    /// When this daemon began serving. A tick before it is one no daemon was there to fire.
    started_at: DateTime<Utc>,
}

impl ActorService {
    pub fn new(
        store: Arc<Store>,
        registry: Arc<Registry>,
        timezone: String,
        schedule_notify: Arc<Notify>,
        started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            store,
            registry,
            timezone,
            schedule_notify,
            started_at,
        }
    }

    /// The Actors of a live Workspace: the ones with a current Session first, then the ones
    /// asking for attention, each by name.
    pub fn list(
        &self,
        workspace: &str,
        kind: Option<&str>,
        include_deleted: bool,
    ) -> Result<Vec<Actor>, ActorError> {
        if !self
            .store
            .get_workspace(workspace)?
            .is_some_and(|workspace| workspace.deleted_at.is_none())
        {
            return Err(ActorError::WorkspaceNotFound(workspace.to_owned()));
        }
        let kind = kind
            .map(str::parse::<ActorKind>)
            .transpose()
            .map_err(ActorError::Invalid)?;
        let mut actors = self.store.list_actors(workspace, kind, include_deleted)?;
        for actor in &mut actors {
            self.mark_overdue(actor)?;
        }
        actors.sort_by(|left, right| {
            actor_rank(left)
                .cmp(&actor_rank(right))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.created_at.cmp(&right.created_at))
        });
        Ok(actors)
    }

    pub fn get(&self, workspace: &str, actor: &str) -> anyhow::Result<Option<Actor>> {
        self.store
            .get_actor(workspace, actor)?
            .map(|actor| self.with_overdue(actor))
            .transpose()
    }

    pub fn get_by_id(&self, actor: &str) -> anyhow::Result<Option<Actor>> {
        self.store
            .get_actor_by_id(actor)?
            .map(|actor| self.with_overdue(actor))
            .transpose()
    }

    fn with_overdue(&self, mut actor: Actor) -> anyhow::Result<Actor> {
        self.mark_overdue(&mut actor)?;
        Ok(actor)
    }

    /// Marks the latest tick that should have fired by now and did not. Only a tick after
    /// this daemon began serving and after the Actor last changed counts, since those
    /// before are ones the Scheduler never planned.
    fn mark_overdue(&self, actor: &mut Actor) -> anyhow::Result<()> {
        let Some(schedule) = actor.schedule.as_mut() else {
            return Ok(());
        };
        if !schedule.enabled || actor.deleted_at.is_some() {
            return Ok(());
        }
        let Ok(Some(planned)) = last_run_at_or_before(
            &schedule.cron,
            &schedule.timezone,
            Utc::now() - OVERDUE_AFTER,
        ) else {
            return Ok(());
        };
        let since = [
            schedule.enabled_at.as_deref(),
            Some(actor.updated_at.as_str()),
        ]
        .into_iter()
        .flatten()
        .filter_map(|at| time::parse(at).ok())
        .fold(self.started_at, DateTime::max);
        if time::parse(&planned).is_ok_and(|instant| instant <= since) {
            return Ok(());
        }
        if !self.store.tick_recorded(&actor.id, &planned)? {
            schedule.overdue_tick = Some(planned);
        }
        Ok(())
    }

    pub fn create(
        &self,
        workspace: &str,
        name: &str,
        declaration: ActorDeclaration<'_>,
        caller: Option<&serde_json::Value>,
    ) -> Result<Actor, ActorError> {
        let ActorDeclaration {
            kind,
            launch,
            schedule,
            grants,
            accept_from,
            writer,
        } = declaration;
        if !valid_key(name) {
            return Err(ActorError::Invalid(
                "Actor names must match ^[a-z0-9][a-z0-9-]*$".to_owned(),
            ));
        }
        validate_accept_from(accept_from)?;
        let launch = self
            .admit_launch(kind, launch, caller)
            .map_err(ActorError::Invalid)?;
        let schedule = schedule
            .map(|schedule| self.resolve_schedule(schedule))
            .transpose()?;
        let actor = self.store.create_actor_as(
            workspace,
            name,
            kind,
            &launch,
            schedule.as_ref(),
            grants,
            accept_from,
            writer,
        )?;
        self.schedule_notify.notify_one();
        Ok(actor)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &self,
        workspace: &str,
        actor: &str,
        rename: Option<&str>,
        launch: Option<LaunchSpecPatch>,
        schedule: Option<SchedulePatch>,
        grants: Option<&BTreeSet<Grant>>,
        accept_from: Option<Option<&BTreeSet<String>>>,
        caller: Option<&serde_json::Value>,
        writer: Writer,
    ) -> Result<Actor, ActorError> {
        let launch = launch.filter(|patch| !patch.is_empty());
        let schedule = schedule.filter(|patch| {
            patch.cron.is_some() || patch.timezone.is_some() || patch.enabled.is_some()
        });
        if rename.is_none()
            && launch.is_none()
            && schedule.is_none()
            && grants.is_none()
            && accept_from.is_none()
        {
            return Err(ActorError::Invalid(
                "Actor update must contain a name, launchSpec, schedule, grants or acceptFrom field"
                    .to_owned(),
            ));
        }
        if rename.is_some_and(|name| !valid_key(name)) {
            return Err(ActorError::Invalid(
                "Actor names must match ^[a-z0-9][a-z0-9-]*$".to_owned(),
            ));
        }
        validate_accept_from(accept_from.flatten())?;
        if let Some(cron) = schedule.as_ref().and_then(|patch| patch.cron.as_ref()) {
            validate_cron(cron).map_err(ActorError::Invalid)?;
        }
        if let Some(timezone) = schedule.as_ref().and_then(|patch| patch.timezone.as_ref()) {
            validate_timezone(timezone).map_err(ActorError::Invalid)?;
        }
        // The Execution Provider resolves the merged launchSpec before the write, which keeps
        // it only while the Actor still holds the launchSpec the patch was merged over. A write
        // refused that way lost to another update that committed, so the patch merges again.
        let actor = loop {
            let resolved = match &launch {
                Some(patch) => {
                    let current = self.store.get_actor(workspace, actor)?.ok_or_else(|| {
                        ActorWriteError::ActorNotFound {
                            workspace: workspace.to_owned(),
                            actor: actor.to_owned(),
                        }
                    })?;
                    let merged = current
                        .launch_spec
                        .clone()
                        .unwrap_or_default()
                        .apply_patch(patch);
                    let resolved = self
                        .admit_launch(current.kind, &merged, caller)
                        .map_err(ActorWriteError::InvalidLaunch)?;
                    Some((current.launch_spec, resolved))
                }
                None => None,
            };
            let update = resolved.as_ref().map(|(expected, resolved)| LaunchUpdate {
                expected: expected.as_ref(),
                resolved,
            });
            match self.store.update_actor_as(
                workspace,
                actor,
                ActorPatch {
                    rename,
                    launch: update,
                    schedule: schedule.as_ref(),
                    grants,
                    accept_from,
                },
                &self.timezone,
                writer,
            ) {
                Err(ActorWriteError::LaunchChanged(_)) => {}
                result => break result?,
            }
        };
        // The Scheduler plans ticks by name, so a rename plans them again as a Schedule does.
        if schedule.is_some() || rename.is_some() {
            self.schedule_notify.notify_one();
        }
        Ok(actor)
    }

    pub fn delete(
        &self,
        workspace: &str,
        actor: &str,
        writer: Writer,
    ) -> Result<Actor, ActorWriteError> {
        let actor = self.store.delete_actor_as(workspace, actor, writer)?;
        self.schedule_notify.notify_one();
        Ok(actor)
    }

    /// A launchSpec is accepted when its shape holds, an Actor of `kind` can start from it,
    /// the Plugin names it chooses are installed, and the Execution Provider's `resolve` fills
    /// in the options the caller left out. What it answers is the declaration the Actor keeps.
    fn admit_launch(
        &self,
        kind: ActorKind,
        launch: &LaunchSpec,
        caller: Option<&serde_json::Value>,
    ) -> Result<LaunchSpec, String> {
        launch.validate()?;
        launch.validate_kind(kind)?;
        launch
            .validate_installed(self.registry.as_ref())
            .map_err(|refusal| refusal.to_string())?;
        let node = launch.node.as_deref().unwrap_or_default();
        if !NODES.contains(&node) {
            return Err(format!("Node '{node}' is unknown"));
        }
        let provider = launch.provider.as_deref().unwrap_or_default();
        let resolved = self
            .registry
            .provider(provider)
            .expect("the Provider is installed")
            .resolve(&ResolveRequest {
                options: launch.provider_options.clone().unwrap_or_default(),
                caller: caller.cloned(),
            })
            .map_err(|error| format!("{error:#}"))?;
        Ok(LaunchSpec {
            provider_options: Some(resolved.options),
            ..launch.clone()
        })
    }

    fn resolve_schedule(
        &self,
        request: ScheduleRequest,
    ) -> Result<ScheduleDeclaration, ActorError> {
        validate_cron(&request.cron).map_err(ActorError::Invalid)?;
        let timezone = request.timezone.unwrap_or_else(|| self.timezone.clone());
        validate_timezone(&timezone).map_err(ActorError::Invalid)?;
        Ok(ScheduleDeclaration {
            cron: request.cron,
            timezone,
        })
    }
}

fn actor_rank(actor: &Actor) -> u8 {
    if actor.current_session.is_some() {
        0
    } else if actor.attention {
        1
    } else {
        2
    }
}

/// The senders an Actor accepts are Actor names.
fn validate_accept_from(accept_from: Option<&BTreeSet<String>>) -> Result<(), ActorError> {
    match accept_from
        .into_iter()
        .flatten()
        .find(|name| !valid_key(name))
    {
        Some(name) => Err(ActorError::Invalid(format!(
            "acceptFrom names Actors, which must match ^[a-z0-9][a-z0-9-]*$; got '{name}'"
        ))),
        None => Ok(()),
    }
}
