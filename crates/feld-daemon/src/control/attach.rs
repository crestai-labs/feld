use crate::store::AttachedExecution;
use feld_core::model::{ObservationKind, Placement};
use feld_core::plugin::protocol::ObserveRequest;

use std::sync::Arc;

use super::{AttachResult, ControlError, SessionController};

impl SessionController {
    pub async fn attach(
        self: &Arc<Self>,
        workspace: &str,
        actor: &str,
        placement: Placement,
        runtime: &str,
        runtime_telemetry_prepared: bool,
    ) -> std::result::Result<AttachResult, ControlError> {
        let (workspace, actor, runtime) =
            (workspace.to_owned(), actor.to_owned(), runtime.to_owned());
        self.operate(move |controller| {
            controller.attach_locked(
                &workspace,
                &actor,
                placement,
                &runtime,
                runtime_telemetry_prepared,
            )
        })
        .await
    }

    fn attach_locked(
        &self,
        workspace: &str,
        actor: &str,
        placement: Placement,
        runtime: &str,
        runtime_telemetry_prepared: bool,
    ) -> std::result::Result<AttachResult, ControlError> {
        let adapter = self.runtime(runtime)?;
        let provider = self.provider(&placement.provider)?;
        if placement.node != "local" {
            return Err(ControlError::NodeNotLocal);
        }
        let observed = provider
            .observe(&ObserveRequest {
                placements: vec![placement.placement.clone()],
            })
            .map_err(|error| {
                ControlError::Placement(format!(
                    "{} could not observe the attached Placement: {error:#}",
                    placement.provider
                ))
            })?
            .results
            .into_iter()
            .next()
            .ok_or_else(|| ControlError::ObserveMismatch {
                provider: placement.provider.clone(),
                answered: 0,
            })?;
        if !matches!(
            observed.observation.kind,
            ObservationKind::Running | ObservationKind::Relocated
        ) {
            return Err(ControlError::Placement(format!(
                "attached Placement is not running: {}",
                observed.observation.detail.unwrap_or_default()
            )));
        }
        let admitted = self.store.admit_attach(
            workspace,
            actor,
            &placement,
            AttachedExecution {
                runtime,
                label: &adapter.manifest().label,
            },
            self.collector_endpoint().is_some(),
            runtime_telemetry_prepared,
        )?;
        Ok(AttachResult {
            session: admitted.pending.session,
            token: admitted.pending.token,
            reattached: admitted.reattached,
        })
    }
}
