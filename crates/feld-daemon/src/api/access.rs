//! What a caller may reach. An Admin token reaches everything. A Session token reaches its own
//! Actor and Sessions, the state of the Actors and Sessions of its Workspace, and what the
//! grants of its Actor open on the other Actors of that Workspace.

use axum::http::StatusCode;

use feld_core::model::{Actor, ActorKind, ActorReference, Grant, LaunchSpec, Requester, Session};

use crate::console::SubAgentSession;
use crate::store::Writer;

use super::auth::AuthContext;
use super::error::{ApiError, error};

impl AuthContext {
    pub(super) fn is_admin(&self) -> bool {
        matches!(self.requester, Requester::Admin)
    }

    /// Who an Actor write is by, which the store checks again in the write's transaction.
    pub(super) fn writer(&self) -> Writer {
        if self.is_admin() {
            Writer::Admin
        } else {
            Writer::Session {
                agents: self.has(Grant::ManageAgent),
                jobs: self.has(Grant::ManageJob),
            }
        }
    }

    /// Hides from a Session token how another Actor starts: its launchSpec, which may hold
    /// secrets in `env` and the arguments, and the command of its Sessions.
    pub(super) fn conceal_actor(&self, actor: &mut Actor) {
        if self.is_admin() || self.owns(&actor.id) {
            return;
        }
        actor.launch_spec = None;
        actor
            .sessions
            .iter_mut()
            .chain(actor.current_session.as_mut())
            .chain(actor.latest_session.as_mut())
            .for_each(|session| session.execution.command = None);
    }

    /// Hides the command of another Actor's Session from a Session token.
    pub(super) fn conceal_session(&self, session: &mut Session) {
        if !self.is_admin() && !self.owns(&session.actor_id) {
            session.execution.command = None;
        }
    }

    /// Hides the command of another Actor's Sub Agent Session from a Session token.
    pub(super) fn conceal_sub_agent(&self, sub_agent: &mut SubAgentSession) {
        let own = self.own_actor().is_some_and(|(_, workspace, name)| {
            workspace == sub_agent.workspace && name == sub_agent.actor
        });
        if !self.is_admin() && !own {
            sub_agent.execution.command = None;
        }
    }

    /// The Actor of the Session token: its `id`, Workspace and name.
    fn own_actor(&self) -> Option<(&str, &str, &str)> {
        match &self.requester {
            Requester::Admin => None,
            Requester::Actor {
                id,
                workspace,
                name,
            } => Some((id, workspace, name)),
        }
    }

    /// The name of the Actor of the Session token.
    pub(super) fn own_name(&self) -> Option<&str> {
        self.own_actor().map(|(_, _, name)| name)
    }

    /// Whether the Actor is the one of the Session token.
    pub(super) fn owns(&self, actor_id: &str) -> bool {
        self.own_actor().is_some_and(|(id, _, _)| id == actor_id)
    }

    /// Refuses a Session token, for an operation of the Admin alone.
    pub(super) fn require_admin(&self, what: &str) -> Result<(), ApiError> {
        if self.is_admin() {
            return Ok(());
        }
        Err(error(
            StatusCode::FORBIDDEN,
            &format!("{what} requires the local token or a browser token"),
        ))
    }

    /// Whether the caller reaches the Workspace: an Admin token every one, a Session token
    /// the one of its Session.
    pub(super) fn reaches(&self, workspace: &str) -> bool {
        self.own_actor()
            .is_none_or(|(_, own_workspace, _)| own_workspace == workspace)
    }

    pub(super) fn require_workspace(&self, workspace: &str) -> Result<(), ApiError> {
        if self.reaches(workspace) {
            return Ok(());
        }
        Err(error(
            StatusCode::FORBIDDEN,
            "a Session token reaches only the Workspace of its Session",
        ))
    }

    /// The Workspace a listing reads: the one asked for, which a Session token must reach,
    /// and the one of the Session for a Session token that asks for none.
    pub(super) fn scope<'a>(&'a self, asked: Option<&'a str>) -> Result<Option<&'a str>, ApiError> {
        match (asked, self.own_actor()) {
            (Some(workspace), _) => self.require_workspace(workspace).map(|()| Some(workspace)),
            (None, Some((_, workspace, _))) => Ok(Some(workspace)),
            (None, None) => Ok(None),
        }
    }

    pub(super) fn has(&self, grant: Grant) -> bool {
        self.is_admin() || self.grants.contains(&grant)
    }

    /// Requires the grant on the other Actors of the Workspace.
    pub(super) fn require_grant(&self, workspace: &str, grant: Grant) -> Result<(), ApiError> {
        self.require_workspace(workspace)?;
        if self.has(grant) {
            return Ok(());
        }
        Err(error(
            StatusCode::FORBIDDEN,
            &format!("this operation requires the grant `{grant}` on the Actor of the Session"),
        ))
    }

    /// The transcript, the output, the metrics and the records of a Session: those of the
    /// own Actor, and with `read` those of the Workspace.
    pub(super) fn require_read(&self, session: &Session) -> Result<(), ApiError> {
        if self.owns(&session.actor_id) {
            return Ok(());
        }
        self.require_grant(&session.workspace, Grant::Read)
    }

    /// Whether the caller reads the body of a Message: one from or to the own Actor, and
    /// with `read` one to an Actor of the Workspace.
    pub(super) fn reads_message(&self, from: &Requester, to: &ActorReference) -> bool {
        let from_own = matches!(from, Requester::Actor { id, .. } if self.owns(id));
        from_own || self.owns(&to.id) || (self.reaches(&to.workspace) && self.has(Grant::Read))
    }

    /// The stop and the kill of a Session: the Session of the token, and with `control`
    /// those of the Workspace.
    pub(super) fn require_stop(&self, session: &Session) -> Result<(), ApiError> {
        if self.session_id.as_deref() == Some(session.id.as_str()) {
            return Ok(());
        }
        self.require_grant(&session.workspace, Grant::Control)
    }

    /// The create, the update and the delete of an Actor of `kind`, now `target`: with
    /// `manage-agent` or `manage-job`, of an Actor without grants or `trustWorkdir`. Such an
    /// Actor is the Admin's to change, so no Session rewrites a launchSpec that runs with
    /// them.
    pub(super) fn require_manage(
        &self,
        workspace: &str,
        kind: ActorKind,
        target: Option<&Actor>,
    ) -> Result<(), ApiError> {
        let grant = match kind {
            ActorKind::Agent => Grant::ManageAgent,
            ActorKind::Job => Grant::ManageJob,
        };
        self.require_grant(workspace, grant)?;
        if self.is_admin() || target.is_none_or(|actor| !admin_owned(actor)) {
            return Ok(());
        }
        Err(error(
            StatusCode::FORBIDDEN,
            "an Actor with grants or `trustWorkdir` is changed only with the local token or a browser token",
        ))
    }
}

/// Whether only the Admin changes the Actor: it has grants or trusts its workdir.
fn admin_owned(actor: &Actor) -> bool {
    !actor.grants.is_empty()
        || actor
            .launch_spec
            .as_ref()
            .is_some_and(LaunchSpec::trusts_workdir)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use feld_core::model::{ActorKind, ActorReference, Grant, Requester};

    use super::AuthContext;

    fn session(grants: &[Grant]) -> AuthContext {
        AuthContext {
            requester: Requester::Actor {
                id: "actor_lead".to_owned(),
                workspace: "team".to_owned(),
                name: "lead".to_owned(),
            },
            session_id: Some("session_lead".to_owned()),
            token: Some("token".to_owned()),
            browser: None,
            grants: grants.iter().copied().collect(),
        }
    }

    fn admin() -> AuthContext {
        AuthContext {
            requester: Requester::Admin,
            session_id: None,
            token: None,
            browser: None,
            grants: BTreeSet::new(),
        }
    }

    fn to(id: &str, workspace: &str) -> ActorReference {
        ActorReference {
            id: id.to_owned(),
            workspace: workspace.to_owned(),
            name: "someone".to_owned(),
        }
    }

    #[test]
    fn a_session_token_reaches_its_workspace_alone() {
        let caller = session(&Grant::ALL);
        assert!(caller.reaches("team"));
        assert!(!caller.reaches("other"));
        assert_eq!(caller.scope(None).ok(), Some(Some("team")));
        assert!(
            caller
                .scope(Some("other"))
                .is_err_and(|refused| refused.status == 403)
        );
        assert!(caller.require_grant("other", Grant::Read).is_err());
        assert_eq!(admin().scope(None).ok(), Some(None));
        assert!(admin().require_grant("other", Grant::Control).is_ok());
    }

    #[test]
    fn a_message_body_is_read_by_its_ends_or_with_read() {
        let own = Requester::Actor {
            id: "actor_lead".to_owned(),
            workspace: "team".to_owned(),
            name: "lead".to_owned(),
        };
        let caller = session(&[]);
        assert!(caller.reads_message(&own, &to("actor_worker", "other")));
        assert!(caller.reads_message(&Requester::Admin, &to("actor_lead", "team")));
        assert!(!caller.reads_message(&Requester::Admin, &to("actor_worker", "team")));
        let reader = session(&[Grant::Read]);
        assert!(reader.reads_message(&Requester::Admin, &to("actor_worker", "team")));
        assert!(!reader.reads_message(&Requester::Admin, &to("actor_worker", "other")));
    }

    #[test]
    fn manage_follows_the_kind_and_leaves_actors_with_grants_to_the_admin() {
        let caller = session(&[Grant::ManageAgent]);
        assert!(
            caller
                .require_manage("team", ActorKind::Agent, None)
                .is_ok()
        );
        assert!(caller.require_manage("team", ActorKind::Job, None).is_err());
        assert!(
            caller
                .require_manage("other", ActorKind::Agent, None)
                .is_err()
        );
    }
}
