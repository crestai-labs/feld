//! `feld workspace`.

use std::process::ExitCode;

use feld_daemon::api::{CreateWorkspaceRequest, WorkspaceList, WorkspaceResponse};

use crate::args::WorkspaceCommand;
use crate::client::{Daemon, percent_encode};
use crate::output::print_table;
use crate::{CliResult, Context};

pub async fn run(ctx: &Context, command: WorkspaceCommand) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    match command {
        WorkspaceCommand::List { include_deleted } => {
            let listing: WorkspaceList = daemon
                .get(&format!("/v1/workspaces?includeDeleted={include_deleted}"))
                .await?;
            ctx.print(&listing, |listing| {
                let rows: Vec<Vec<String>> = listing
                    .workspaces
                    .iter()
                    .map(|workspace| {
                        let mut row = vec![
                            workspace.key.clone(),
                            workspace.actor_count.to_string(),
                            ctx.local_time(&workspace.created_at),
                        ];
                        if include_deleted {
                            row.push(ctx.local_time_or_dash(workspace.deleted_at.as_deref()));
                        }
                        row
                    })
                    .collect();
                if include_deleted {
                    print_table(&["KEY", "ACTORS", "CREATED", "DELETED"], &rows);
                } else {
                    print_table(&["KEY", "ACTORS", "CREATED"], &rows);
                }
            })?;
        }
        WorkspaceCommand::Delete { key } => {
            let response: WorkspaceResponse = daemon
                .delete(&format!("/v1/workspaces/{}", percent_encode(&key)))
                .await?;
            ctx.print(&response, |response| {
                println!("Workspace deleted.");
                println!("  key:        {}", response.workspace.key);
                println!(
                    "  deleted_at: {}",
                    ctx.local_time_or_dash(response.workspace.deleted_at.as_deref())
                );
            })?;
        }
        WorkspaceCommand::Get { key } => {
            let response: WorkspaceResponse = daemon
                .get(&format!("/v1/workspaces/{}", percent_encode(&key)))
                .await?;
            ctx.print(&response, |response| {
                let workspace = &response.workspace;
                println!("Workspace");
                println!("  key:        {}", workspace.key);
                println!("  actors:     {}", workspace.actor_count);
                println!("  created_at: {}", ctx.local_time(&workspace.created_at));
                println!(
                    "  deleted_at: {}",
                    ctx.local_time_or_dash(workspace.deleted_at.as_deref())
                );
            })?;
        }
        WorkspaceCommand::Create { key } => {
            let response: WorkspaceResponse = daemon
                .post_json("/v1/workspaces", &CreateWorkspaceRequest { key })
                .await?;
            ctx.print(&response, |response| {
                println!("Workspace created.");
                println!("  key:        {}", response.workspace.key);
                println!(
                    "  created_at: {}",
                    ctx.local_time(&response.workspace.created_at)
                );
            })?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
