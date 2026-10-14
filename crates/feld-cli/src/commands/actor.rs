//! `feld actor`, except `attach`.

use std::collections::BTreeSet;
use std::process::ExitCode;

use feld_core::model::{Actor, ActorKind};
use feld_core::plugin::Registry;
use feld_daemon::api::{
    ActorList, ActorResponse, CreateActorRequest, CreateScheduleRequest, RestartResponse,
    SessionResponse, StartResponse, UpdateActorRequest, UpdateScheduleRequest,
};

use crate::args::{ActorCommand, ActorCreateArgs, ActorListArgs, ActorUpdateArgs};
use crate::client::{Daemon, actor_path, actors_path, percent_encode, session_path};
use crate::format::actor::{
    accept_from_summary, actor_marks, grants_summary, print_actor_created, print_actor_detail,
    print_actor_updated,
};
use crate::format::session::{
    outcome_label, print_start, print_start_lines, start_exit_code, start_title,
};
use crate::launch::{launch_patch, resolve_launch};
use crate::output::{change_line, kinded, print_json, print_table_or};
use crate::provider_options::ProviderOption;
use crate::{CliResult, Context};

/// How long `feld actor restart` waits for the next generation after the stop is accepted.
const RESTART_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn run(
    ctx: &Context,
    command: ActorCommand,
    options: &[ProviderOption],
) -> CliResult<ExitCode> {
    match command {
        ActorCommand::Create(args) => create(ctx, args, options).await,
        ActorCommand::Update(args) => update(ctx, args, options).await,
        ActorCommand::Delete(target) => delete(ctx, &target.actor).await,
        ActorCommand::Get(args) => get(ctx, &args.target).await,
        ActorCommand::List(args) => list(ctx, args).await,
        ActorCommand::Start(target) => start(ctx, &target.actor).await,
        ActorCommand::Attach(args) => super::attach::run(ctx, args, options).await,
        ActorCommand::Restart(target) => restart(ctx, &target.actor).await,
        ActorCommand::Pause(target) => set_schedule_enabled(ctx, &target.actor, false).await,
        ActorCommand::Resume(target) => set_schedule_enabled(ctx, &target.actor, true).await,
    }
}

/// `feld actor create` creates first and starts afterwards, so a start that is rejected
/// leaves the committed creation for the next command to build on.
async fn create(
    ctx: &Context,
    args: ActorCreateArgs,
    options: &[ProviderOption],
) -> CliResult<ExitCode> {
    let workspace = &ctx.workspace;
    let daemon = Daemon::connect(ctx).await?;
    let registry = Registry::load(&ctx.home, Some(&daemon.api));
    let start = match args.kind {
        ActorKind::Agent => !args.no_start,
        ActorKind::Job => args.start,
    };
    let (launch, caller) = resolve_launch(args.launch, options, &daemon.providers, &registry)?;
    let created: ActorResponse = daemon
        .post_json(
            &actors_path(workspace),
            &CreateActorRequest {
                name: args.name.clone(),
                kind: args.kind,
                launch_spec: launch,
                schedule: args.cron.map(|cron| CreateScheduleRequest {
                    cron,
                    timezone: args.timezone,
                }),
                grants: (!args.grants.is_empty()).then(|| args.grants.into_iter().collect()),
                accept_from: (!args.accept_from.is_empty())
                    .then(|| args.accept_from.into_iter().collect()),
                caller,
            },
        )
        .await?;
    // The daemon keeps a live Actor's `createdAt` when a create replaces its declaration.
    let replaced = created.actor.created_at != created.actor.updated_at;
    if !start {
        ctx.print(&created, |created| {
            print_actor_created(
                ctx,
                if replaced {
                    "Actor replaced."
                } else {
                    "Actor created."
                },
                &created.actor,
            );
        })?;
        print_notes(ctx, [network_note(&created.actor)]);
        return Ok(ExitCode::SUCCESS);
    }
    // A sandbox that reaches the socket of its pane is told before the Session starts.
    if let Some(note) = network_note(&created.actor) {
        eprintln!("note: {note}");
    }
    let started: StartResponse = daemon
        .post(&format!("{}/start", actor_path(workspace, &args.name)))
        .await
        .map_err(|error| error.after_commit("Actor declaration"))?;
    ctx.print(
        &serde_json::json!({
            "actor": created.actor,
            "session": started.session,
            "replacedSession": started.replaced_session,
        }),
        |_| {
            print_actor_created(
                ctx,
                start_title(
                    &started.session,
                    if replaced {
                        "Actor replaced and session started."
                    } else {
                        "Actor created and session started."
                    },
                    "Actor declaration committed; Session failed to start.",
                ),
                &created.actor,
            );
            print_start_lines(ctx, &started.session, started.replaced_session.as_deref());
        },
    )?;
    Ok(start_exit_code(&started.session))
}

/// The note of a Codex declaration whose sandbox reaches the socket of its pane.
fn network_note(actor: &Actor) -> Option<String> {
    use crate::codex_sandbox;
    let config = codex_sandbox::config_path()
        .and_then(|path| codex_sandbox::config_sandbox(&path))
        .unwrap_or_default();
    codex_sandbox::network_note(actor.launch_spec.as_ref()?, &config)
}

/// Notes on stderr beside JSON, as the last lines of the block otherwise.
fn print_notes(ctx: &Context, notes: impl IntoIterator<Item = Option<String>>) {
    for note in notes.into_iter().flatten() {
        if ctx.json {
            eprintln!("note: {note}");
        } else {
            change_line("note:", &note);
        }
    }
}

async fn update(
    ctx: &Context,
    args: ActorUpdateArgs,
    options: &[ProviderOption],
) -> CliResult<ExitCode> {
    let launch_arguments = args.launch.clone();
    let daemon = Daemon::connect(ctx).await?;
    let registry = Registry::load(&ctx.home, Some(&daemon.api));
    let (launch, caller) = launch_patch(args.launch, options, &daemon.providers, &registry)?;
    let launch = (!launch.is_empty()).then_some(launch);
    let schedule =
        (args.cron.is_some() || args.timezone.is_some()).then_some(UpdateScheduleRequest {
            cron: args.cron,
            timezone: args.timezone,
            enabled: None,
        });
    let grants = (args.no_grants || !args.grants.is_empty())
        .then(|| args.grants.into_iter().collect::<BTreeSet<_>>());
    let accept_from = (args.no_accept_from || !args.accept_from.is_empty()).then(|| {
        (!args.no_accept_from).then(|| args.accept_from.into_iter().collect::<BTreeSet<_>>())
    });
    if args.name.is_none()
        && launch.is_none()
        && schedule.is_none()
        && grants.is_none()
        && accept_from.is_none()
    {
        return Err(operational!(
            "feld actor update requires a name, launchSpec, Schedule, grant or accept-from option"
        ));
    }
    let updates_grants = grants.is_some();
    let updates_accept_from = accept_from.is_some();
    let updates_launch = launch.is_some();
    let updates_schedule = schedule.is_some();
    let response: ActorResponse = daemon
        .patch(
            &actor_path(&ctx.workspace, &args.actor),
            &UpdateActorRequest {
                name: args.name,
                launch_spec: launch,
                schedule,
                grants,
                accept_from,
                caller,
            },
        )
        .await?;
    ctx.print(&response, |response| {
        print_actor_updated(
            ctx,
            &response.actor,
            (response.actor.name != args.actor).then_some(args.actor.as_str()),
            updates_launch.then_some(&launch_arguments),
            updates_schedule,
            updates_grants,
            updates_accept_from,
        );
    })?;
    if updates_launch {
        print_notes(ctx, [network_note(&response.actor)]);
    }
    Ok(ExitCode::SUCCESS)
}

async fn delete(ctx: &Context, actor: &str) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let response: ActorResponse = daemon.delete(&actor_path(&ctx.workspace, actor)).await?;
    ctx.print(&response, |response| {
        let actor = &response.actor;
        println!("Actor deleted.");
        change_line("actor:", &kinded(&actor.name, actor.kind));
        change_line("id:", &actor.id);
        change_line(
            "deleted_at:",
            &ctx.local_time_or_dash(actor.deleted_at.as_deref()),
        );
    })?;
    Ok(ExitCode::SUCCESS)
}

/// An Actor ID never matches the name grammar, so the target alone says which it is.
async fn get(ctx: &Context, target: &str) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let response: ActorResponse = if target.starts_with("actor_") {
        daemon
            .get(&format!("/v1/actors/{}", percent_encode(target)))
            .await?
    } else {
        daemon.get_actor(&ctx.workspace, target).await?
    };
    ctx.print(&response, |response| {
        print_actor_detail(ctx, &response.actor)
    })?;
    Ok(ExitCode::SUCCESS)
}

async fn list(ctx: &Context, args: ActorListArgs) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let mut query = Vec::new();
    if let Some(kind) = args.kind {
        query.push(("kind", kind.as_str()));
    }
    if args.include_deleted {
        query.push(("includeDeleted", "true"));
    }
    let path = actors_path(&ctx.workspace);
    let mut listing: ActorList = daemon.get_with(&path, &query).await?;
    if args.refresh {
        for session in listing
            .actors
            .iter()
            .filter_map(|actor| actor.current_session.as_ref())
        {
            let observed: CliResult<SessionResponse> = daemon
                .post(&format!("{}/observe", session_path(&session.id)))
                .await;
            // A Session that closed after the listing has nothing left to observe.
            if let Err(error) = observed
                && daemon.get_session(&session.id).await?.session.end.is_none()
            {
                return Err(error);
            }
        }
        listing = daemon.get_with(&path, &query).await?;
    }
    let total = listing.actors.len();
    if !args.all {
        listing.actors.retain(is_active);
    }
    let listing = ListedActors {
        hidden_count: total - listing.actors.len(),
        listing,
    };
    ctx.print(
        &listing,
        |ListedActors {
             listing,
             hidden_count,
         }| {
            println!("Workspace: {}", listing.workspace);
            let rows: Vec<Vec<String>> = listing
                .actors
                .iter()
                .map(|actor| {
                    let next = actor
                        .schedule
                        .as_ref()
                        .map(|schedule| {
                            if schedule.enabled {
                                ctx.local_time_or_dash(schedule.next_run.as_deref())
                            } else {
                                "paused".to_owned()
                            }
                        })
                        .unwrap_or_else(|| "-".to_owned());
                    let session = actor
                        .current_session
                        .as_ref()
                        .or(actor.latest_session.as_ref());
                    let (session_id, generation, start_kind, outcome, observed) = session.map_or(
                        ("-".to_owned(), "-".to_owned(), "-", "-".to_owned(), "-"),
                        |session| {
                            (
                                session.id.clone(),
                                session.generation.to_string(),
                                session.start_kind.as_str(),
                                outcome_label(session),
                                session
                                    .observation
                                    .as_ref()
                                    .map_or("-", |observation| observation.kind.as_str()),
                            )
                        },
                    );
                    let mut row = vec![
                        actor.name.clone(),
                        actor.kind.to_string(),
                        session_id,
                        generation,
                        start_kind.to_owned(),
                        outcome,
                        observed.to_owned(),
                        actor.waiting_count.to_string(),
                        actor_marks(actor),
                        next,
                        grants_summary(&actor.grants),
                        accept_from_summary(actor.accept_from.as_ref()),
                    ];
                    if args.include_deleted {
                        row.push(ctx.local_time_or_dash(actor.deleted_at.as_deref()));
                    }
                    row
                })
                .collect();
            let mut headers = vec![
                "NAME",
                "KIND",
                "SESSION",
                "GEN",
                "START",
                "OUTCOME",
                "OBSERVED",
                "WAITING",
                "MARKS",
                "NEXT_RUN",
                "GRANTS",
                "ACCEPT_FROM",
            ];
            if args.include_deleted {
                headers.push("DELETED");
            }
            print_table_or("No Actors.", &headers, &rows);
            if *hidden_count > 0 {
                let noun = if *hidden_count == 1 {
                    "Actor"
                } else {
                    "Actors"
                };
                println!("{hidden_count} stopped {noun} not shown (use -a)");
            }
        },
    )?;
    Ok(ExitCode::SUCCESS)
}

/// `feld actor list` without `-a`: the listing plus how many Actors it leaves out.
#[derive(serde::Serialize)]
struct ListedActors {
    #[serde(flatten)]
    listing: ActorList,
    #[serde(rename = "hiddenCount")]
    hidden_count: usize,
}

/// An Actor `feld actor list` shows without `-a`: a live one holding a current Session, or a
/// live Job whose Schedule is enabled.
fn is_active(actor: &Actor) -> bool {
    let scheduled = actor.kind == ActorKind::Job
        && actor
            .schedule
            .as_ref()
            .is_some_and(|schedule| schedule.enabled);
    actor.deleted_at.is_none() && (actor.current_session.is_some() || scheduled)
}

async fn start(ctx: &Context, actor: &str) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let response: StartResponse = daemon
        .post(&format!("{}/start", actor_path(&ctx.workspace, actor)))
        .await?;
    ctx.print(&response, |response| {
        print_start(
            ctx,
            start_title(
                &response.session,
                "Start accepted and committed.",
                "Start accepted and committed; Session failed to start.",
            ),
            &response.session,
            response.replaced_session.as_deref(),
        );
    })?;
    Ok(start_exit_code(&response.session))
}

/// `feld actor restart` returns once the stop is accepted and, when the stopped Session
/// has yet to close, waits for the next generation to show up as the Actor's current one.
async fn restart(ctx: &Context, actor: &str) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let mut response: RestartResponse = daemon
        .post(&format!("{}/restart", actor_path(&ctx.workspace, actor)))
        .await?;
    let deadline = std::time::Instant::now() + RESTART_WAIT;
    while response.session.is_none() && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        response.session = daemon
            .get_actor(&ctx.workspace, actor)
            .await?
            .actor
            .sessions
            .into_iter()
            .filter(|session| session.generation > response.stopped.generation)
            .min_by_key(|session| session.generation);
    }
    if ctx.json {
        print_json(&response)?;
        return Ok(response
            .session
            .as_ref()
            .map_or(ExitCode::SUCCESS, start_exit_code));
    }
    let stopped = &response.stopped;
    let Some(session) = &response.session else {
        println!(
            "Restart accepted and committed; the next generation starts once the Session closes."
        );
        change_line("actor:", &kinded(&stopped.actor, stopped.actor_kind));
        change_line("stopping:", &stopped.id);
        return Ok(ExitCode::SUCCESS);
    };
    println!(
        "{}",
        start_title(
            session,
            "Restart accepted and committed.",
            "Restart accepted and committed; Session failed to start.",
        )
    );
    change_line("actor:", &kinded(&session.actor, session.actor_kind));
    change_line(
        "stopped:",
        &match stopped.exit_code {
            Some(exit_code) => format!(
                "{} ({}, exit_code {exit_code})",
                stopped.id,
                stopped.outcome.as_str()
            ),
            None => kinded(&stopped.id, stopped.outcome.as_str()),
        },
    );
    print_start_lines(ctx, session, Some(&stopped.id));
    Ok(start_exit_code(session))
}

async fn set_schedule_enabled(ctx: &Context, actor: &str, enabled: bool) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let current = daemon.get_actor(&ctx.workspace, actor).await?;
    if current.actor.kind == ActorKind::Job && current.actor.schedule.is_none() {
        return Err(rejected!(
            "Actor '{actor}' has no Schedule; add one with `feld actor update {actor} --cron ...`"
        ));
    }
    let response: ActorResponse = daemon
        .patch(
            &actor_path(&ctx.workspace, actor),
            &UpdateActorRequest {
                schedule: Some(UpdateScheduleRequest {
                    enabled: Some(enabled),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await?;
    if ctx.json {
        print_json(&response)?;
        return Ok(ExitCode::SUCCESS);
    }
    let Some(schedule) = &response.actor.schedule else {
        return Err(operational!(
            "the updated Actor '{actor}' carries no Schedule"
        ));
    };
    if enabled {
        println!("Schedule resumed.");
        change_line(
            "enabled:",
            &format!(
                "yes (since {})",
                ctx.local_time_or_dash(schedule.enabled_at.as_deref())
            ),
        );
        change_line(
            "next_run:",
            &ctx.local_time_or_dash(schedule.next_run.as_deref()),
        );
    } else {
        println!("Schedule paused.");
        change_line(
            "enabled:",
            &format!(
                "no (since {})",
                ctx.local_time_or_dash(schedule.paused_at.as_deref())
            ),
        );
        if let Some(current) = &response.actor.current_session {
            change_line(
                "current:",
                &format!("{} ({}, not touched)", current.id, current.outcome.as_str()),
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}
