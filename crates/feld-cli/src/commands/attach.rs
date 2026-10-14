//! `feld actor attach`.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Context as _;
use feld_core::home::FELD_SESSION_TOKEN_ENV;
use feld_core::model::{
    Actor, ActorKind, ObservationKind, Placement, PlacementProcess, RuntimeIdentity,
};
use feld_core::plugin::protocol::{
    AttachedRequest, Candidate, CandidatesRequest, IdentityRequest, MatchRequest, PrepareRequest,
    ProviderAttachRequest, ProviderAttachResponse,
};
use feld_core::plugin::{
    ExecutionProvider, Plugins, ProviderChoiceError, Registry, RuntimeAdapter,
    provider_not_installed, runtime_not_installed, runtime_self_index_key, self_index_key,
};
use feld_core::process::{self, ProcessSnapshot};
use feld_core::session_token::{
    self as session_token, lock_self_index, sweep_self_index, write_self_index,
};
use feld_daemon::api::{ActorList, AttachActorRequest, AttachResponse, DaemonStatus};
use reqwest::Method;
use serde_json::Value;

use crate::args::AttachArgs;
use crate::client::{Daemon, actor_path, actors_path, admin_outside_sandbox_as};
use crate::format::session::print_attached;
use crate::launch::provider_here;
use crate::output::{change_line, print_json};
use crate::provider_options::{ProviderOption, provider_options};
use crate::{CliError, CliResult, Context};

/// How many generated names an attach tries before it gives up.
const ATTACH_ATTEMPTS: usize = 100;

/// The execution an attach takes, as the Provider and the Runtime Adapter identified it.
struct Identified {
    resolved: ProviderAttachResponse,
    /// The Runtime process in front of the execution.
    process: PlacementProcess,
    /// Whether the Runtime exports its telemetry to this Session, and why not when not.
    telemetry_prepared: bool,
    telemetry_reason: Option<String>,
    notes: Vec<String>,
}

/// A name the attach generated, `<base>-<next>` being the one to try after it.
type Generated = (String, usize);

/// `feld actor attach` follows the order of plugin.md's "Division of work on attach": the Provider
/// identifies the resource and the process in front of it, the Runtime Adapter confirms that
/// process is its Runtime and checks what `feld init` prepared, and only the identified result
/// reaches the daemon.
pub async fn run(
    ctx: &Context,
    args: AttachArgs,
    options: &[ProviderOption],
) -> CliResult<ExitCode> {
    if args.self_ && std::env::var_os(FELD_SESSION_TOKEN_ENV).is_some() {
        return Err(rejected!("--self cannot attach from an owned Feld Session"));
    }
    let daemon = if args.self_ {
        Daemon::connect_local_or(ctx, || outside_sandbox(ctx, &args)).await?
    } else {
        Daemon::connect_local(ctx).await?
    };
    let registry = Registry::load(&ctx.home, Some(&daemon.api));
    let adapter = registry
        .runtime(&args.runtime)
        .ok_or_else(|| CliError::Rejected(runtime_not_installed(&registry, &args.runtime)))?;
    let provider_name =
        provider_here(None, options, &daemon.providers, &registry).map_err(|error| {
            let refused = CliError::Rejected(error.to_string());
            match error {
                ProviderChoiceError::Ambiguous => refused.with_next(run_in_pane(
                    &daemon.providers.join(" or "),
                    &adapter.manifest().label,
                )),
                _ => refused,
            }
        })?;
    let provider = registry
        .provider(&provider_name)
        .ok_or_else(|| CliError::Rejected(provider_not_installed(&registry, &provider_name)))?;
    let status: DaemonStatus = daemon.get("/v1/daemon/status").await?;
    let Target {
        identified,
        index_key,
    } = target(
        ctx,
        &daemon,
        AttachPlugins {
            provider: provider.as_ref(),
            provider_name: &provider_name,
            adapter: adapter.as_ref(),
        },
        &args,
        options,
        status.otlp,
    )
    .await?;
    // The self index entry of this place, held from here so that two attaches to the same
    // place never write the index at the same time.
    let index_lock = lock_self_index(&ctx.home, &index_key)?;
    let Identified {
        resolved,
        process,
        telemetry_prepared,
        telemetry_reason,
        mut notes,
    } = identified;
    let (actor, generated) =
        actor_name(ctx, &daemon, args.name, args.self_, &process, &resolved).await?;

    // 5. the daemon receives the identified result.
    let request = AttachActorRequest {
        placement: Placement {
            provider: provider_name,
            node: "local".to_owned(),
            workdir: resolved.workdir,
            placement: resolved.placement,
        },
        runtime: args.runtime,
        runtime_telemetry_prepared: telemetry_prepared,
    };
    let response = commit(ctx, &daemon, &request, actor, generated, &process).await?;
    write_self_index(
        &ctx.home,
        &response.session.id,
        &response.session.actor,
        &process,
        &response.token,
        &index_lock,
    )
    .with_context(|| {
        format!(
            "attach committed for Session '{}', but its token cache could not be saved",
            response.session.id
        )
    })?;
    // The entries of Runtimes that have ended are a cache of closed Sessions; losing the
    // sweep costs nothing but leaving them for the next attach.
    let _ = sweep_self_index(&ctx.home, &index_lock, &response.session.id);

    // Outside the Runtime, the process the Provider found in front of the execution tells
    // the identity instead of this CLI's environment.
    let runtime = (!args.self_).then_some(resolved.process).flatten();
    let delivered = deliver_identity(&daemon, adapter.as_ref(), runtime, &response.token).await?;
    notes.extend(delivered.notes);
    // 7. what runs beside the Runtime starts with this token, without waiting for an event of
    // a Runtime that waits for its next input.
    match adapter.attached(&AttachedRequest {
        session: response.session.id.clone(),
        token: response.token.clone(),
        identity: delivered.identity,
    }) {
        Ok(attached) => notes.extend(attached.notes),
        Err(error) => notes.push(format!(
            "what runs beside the Runtime did not start ({error:#}); it starts once a hook fires."
        )),
    }
    if ctx.json {
        print_json(&response)?;
        for note in &notes {
            eprintln!("note: {note}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    print_attached(
        ctx,
        if response.reattached {
            "Session token rotated. Nothing was started or signalled."
        } else {
            "Attach accepted and committed. Nothing was started or signalled."
        },
        &response.session,
    );
    if !telemetry_prepared {
        change_line(
            "telemetry:",
            &format!(
                "unavailable · {}",
                telemetry_reason.as_deref().unwrap_or(
                    "the Runtime did not start in a shape that exports telemetry to this daemon"
                )
            ),
        );
    }
    for note in &notes {
        change_line("note:", note);
    }
    Ok(ExitCode::SUCCESS)
}

/// The two Plugins an attach runs, with the name the Provider is installed under.
#[derive(Clone, Copy)]
struct AttachPlugins<'a> {
    provider: &'a dyn ExecutionProvider,
    provider_name: &'a str,
    adapter: &'a dyn RuntimeAdapter,
}

/// What an attach takes, and the self index entry its token goes to.
struct Target {
    identified: Identified,
    index_key: String,
}

/// The execution an attach takes. The Provider's options name it from outside. `--self`
/// takes the Runtime this CLI descends from, in front of the pane the Provider's `self`
/// answers or the options name; the tools of that Runtime find its token by the place. A
/// Runtime that runs the caller's conversation in a process apart from the one in front of
/// the pane answers the conversation with its own `self` instead, and then a person names
/// the pane and the tools of that conversation find the token by the conversation.
async fn target(
    ctx: &Context,
    daemon: &Daemon,
    plugins: AttachPlugins<'_>,
    args: &AttachArgs,
    options: &[ProviderOption],
    collector: Option<String>,
) -> CliResult<Target> {
    let given = provider_options(plugins.provider_name, options)?;
    if !args.self_ {
        let selector = given.ok_or_else(|| {
            rejected!("attach requires --self or a Provider selector such as --tmux-pane")
        })?;
        let identified = identify(plugins, selector, None, collector)?;
        return Ok(Target {
            index_key: self_index_key(plugins.provider_name, &identified.resolved.selector),
            identified,
        });
    }
    let selector = match &given {
        Some(selector) => Some(selector.clone()),
        None => plugins.provider.current()?.selector,
    };
    let refused = match selector {
        Some(selector) => match identify(
            plugins,
            selector,
            Some(std::process::id()),
            collector.clone(),
        ) {
            Ok(identified) => {
                return Ok(Target {
                    index_key: self_index_key(plugins.provider_name, &identified.resolved.selector),
                    identified,
                });
            }
            Err(refused) => refused,
        },
        None => rejected!("--self requires a {} environment", plugins.provider_name).with_next(
            run_in_pane(plugins.provider_name, &plugins.adapter.manifest().label),
        ),
    };
    // An Adapter that answers no `self` runs its tools under the process in front of the pane.
    let Some(conversation) = plugins.adapter.current().ok() else {
        return Err(refused);
    };
    let Some(selector) = conversation.selector else {
        return Err(refused);
    };
    // The conversation is the caller's only while a process of the Runtime runs it.
    if !session_token::runs_under(plugins.adapter, std::process::id())
        .context("read the caller's process ancestry")?
    {
        return Err(refused);
    }
    let Some(pane) = given else {
        return Err(choose(ctx, daemon, plugins, args, &refused, &conversation.notes).await);
    };
    let mut identified = identify(plugins, pane, None, collector)?;
    identified.notes.push(format!(
        "{} is the pane given with --self; {} runs this conversation in a process outside it",
        identified.resolved.summary,
        plugins.adapter.manifest().label
    ));
    Ok(Target {
        index_key: runtime_self_index_key(&args.runtime, &selector),
        identified,
    })
}

/// The refusal of a `--self` whose pane only a person can tell: the panes of the caller's
/// directory with this Runtime in front of them, each with the attach that takes it. A pane
/// is never picked for the person, however few there are.
async fn choose(
    ctx: &Context,
    daemon: &Daemon,
    plugins: AttachPlugins<'_>,
    args: &AttachArgs,
    refused: &CliError,
    notes: &[String],
) -> CliError {
    let label = &plugins.adapter.manifest().label;
    let directory = std::env::current_dir()
        .map(|directory| directory.display().to_string())
        .unwrap_or_default();
    let mut message = format!(
        "{label} runs this conversation in a process outside the pane, so the pane cannot be told from here ({refused:#})."
    );
    match candidates(ctx, daemon, plugins, &directory).await {
        Ok(found) if found.is_empty() => {
            message.push_str(&format!(
                "\nNo {} pane in {directory} has {label} in front of it. The attach takes the pane as:\n  {}",
                plugins.provider_name,
                retry(ctx, plugins.provider_name, args, None)
            ));
        }
        Ok(found) => {
            message.push_str(&format!(
                "\nThese {} panes in {directory} have {label} in front of them, each with the attach that takes it:",
                plugins.provider_name
            ));
            for (candidate, holder) in found {
                message.push_str(&format!(
                    "\n  {}  {}  started {}",
                    candidate.summary,
                    process::command_line(&candidate.process.argv),
                    candidate.process.started_at
                ));
                if let Some(holder) = holder {
                    message.push_str(&format!("  (the Runtime of Actor {holder})"));
                }
                message.push_str(&format!(
                    "\n    {}",
                    retry(ctx, plugins.provider_name, args, Some(&candidate.options))
                ));
            }
        }
        Err(error) => {
            message.push_str(&format!(
                "\nThe {} panes could not be listed ({error:#}). The attach takes the pane as:\n  {}",
                plugins.provider_name,
                retry(ctx, plugins.provider_name, args, None)
            ));
        }
    }
    for note in notes {
        message.push_str(&format!("\n{note}"));
    }
    CliError::Rejected(message).with_next(format!(
        "ask the person which pane shows this {label}, and attach again with its line below"
    ))
}

/// The step of a `--self` that found no pane with the Runtime in front of it.
fn run_in_pane(providers: &str, runtime: &str) -> String {
    format!(
        "run it inside the {providers} pane where {runtime} runs; `feld doctor` checks the rest"
    )
}

/// The Provider's resources in `directory` with this Runtime in front of them, each with the
/// Actor whose current Session already holds that Runtime.
async fn candidates(
    ctx: &Context,
    daemon: &Daemon,
    plugins: AttachPlugins<'_>,
    directory: &str,
) -> anyhow::Result<Vec<(Candidate, Option<String>)>> {
    let found = plugins.provider.candidates(&CandidatesRequest {
        selector: plugins.provider.current()?.selector,
    })?;
    let listing = list_actors(ctx, daemon)
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let directory = Path::new(directory).canonicalize().ok();
    let mut chosen = Vec::new();
    for candidate in found.candidates {
        if Path::new(&candidate.workdir).canonicalize().ok() != directory {
            continue;
        }
        let matched = plugins.adapter.match_process(&MatchRequest {
            process: candidate.process.clone(),
            collector: None,
        })?;
        if !matched.matched {
            continue;
        }
        let holder =
            holder(&listing, &candidate.process.incarnation()).map(|actor| actor.name.clone());
        chosen.push((candidate, holder));
    }
    Ok(chosen)
}

/// The `--self` attach that takes the resource `options` names, or with a placeholder for it.
fn retry(ctx: &Context, provider: &str, args: &AttachArgs, options: Option<&Value>) -> String {
    attach_line(ctx, provider, args, options, true)
}

/// The attach that takes the resource `options` names, or with a placeholder for it. One
/// without `--self` names its Actor, with a placeholder when `args` names none.
fn attach_line(
    ctx: &Context,
    provider: &str,
    args: &AttachArgs,
    options: Option<&Value>,
    self_: bool,
) -> String {
    let mut line = "feld actor attach".to_owned();
    match &args.name {
        Some(name) => line.push_str(&format!(" {}", process::quote_for_display(name))),
        None if !self_ => line.push_str(" <name>"),
        None => {}
    }
    let mut words = vec![
        "--workspace".to_owned(),
        ctx.workspace.clone(),
        "--runtime".to_owned(),
        args.runtime.clone(),
    ];
    if self_ {
        words.push("--self".to_owned());
    }
    line.push_str(&format!(" {}", process::command_line(&words)));
    match options.and_then(Value::as_object) {
        Some(options) => {
            for (key, value) in options {
                if let Some(value) = value.as_str() {
                    line.push_str(&format!(
                        " --{provider}-{key} {}",
                        process::quote_for_display(value)
                    ));
                }
            }
        }
        None => line.push_str(&format!(" --{provider}-pane <pane>")),
    }
    line
}

/// The refusal of a `--self` inside a sandbox, with the attach of the caller's pane that runs
/// outside it (`spool.md`).
fn outside_sandbox(ctx: &Context, args: &AttachArgs) -> CliError {
    let registry = Registry::load(&ctx.home, None);
    let here = registry.providers().into_iter().find_map(|provider| {
        let selector = registry
            .provider(&provider.name)?
            .current()
            .ok()?
            .selector?;
        Some((provider.name, selector))
    });
    let (provider, selector) = match &here {
        Some((provider, selector)) => (provider.as_str(), Some(selector)),
        None => (registry.sole_provider().unwrap_or("<provider>"), None),
    };
    admin_outside_sandbox_as(&attach_line(ctx, provider, args, selector, false))
}

/// Steps 1-4: the execution the selector names, and the Runtime in front of it.
fn identify(
    plugins: AttachPlugins<'_>,
    selector: Value,
    caller_pid: Option<u32>,
    collector: Option<String>,
) -> CliResult<Identified> {
    // 1-2. the Provider identifies the resource and the process in front of it.
    let AttachPlugins {
        provider, adapter, ..
    } = plugins;
    let resolved = provider
        .attach(&ProviderAttachRequest { selector })
        .map_err(|error| rejected!("{error:#}"))?;
    let process = resolved
        .process
        .clone()
        .ok_or_else(|| rejected!("the selected execution has no Runtime process"))?;
    // `--self` takes the Runtime this CLI runs inside, so the process the Provider found
    // has to be one this CLI descends from.
    if let Some(caller_pid) = caller_pid
        && !feld_core::process::is_descendant(caller_pid, process.pid)
            .context("read the caller's process ancestry")?
    {
        return Err(rejected!(
            "the caller is not a descendant of the Runtime process in front of the execution"
        )
        .with_next(run_in_pane(
            plugins.provider_name,
            &adapter.manifest().label,
        )));
    }

    // 3. the Runtime Adapter confirms that process is the Runtime that was asked for.
    let matched = adapter.match_process(&MatchRequest {
        process: process.clone(),
        collector: collector.clone(),
    })?;
    if !matched.matched {
        let refused = rejected!(
            "the process in front of the execution is not {}",
            adapter.manifest().label
        );
        // `--self` runs from the Runtime it takes, so the caller sits in another pane.
        return Err(match caller_pid {
            Some(_) => refused.with_next(run_in_pane(
                plugins.provider_name,
                &adapter.manifest().label,
            )),
            None => refused,
        });
    }

    // 4. the Runtime Adapter checks what `feld init` prepared in the Runtime a person
    // started themselves, and writes nothing there.
    let prepared = adapter.prepare(&PrepareRequest {
        apply: false,
        collector,
    })?;
    // A Runtime that started without what its telemetry needs exports none to this
    // Session; the daemon records that on the Session and its metrics say so.
    let (telemetry_prepared, telemetry_reason) = if prepared.telemetry_prepared == Some(false) {
        (
            false,
            prepared.telemetry_reason.or(matched.telemetry_reason),
        )
    } else if matched.telemetry_prepared == Some(false) {
        (false, matched.telemetry_reason)
    } else {
        (true, None)
    };
    Ok(Identified {
        resolved,
        // The Runtime process the Placement names, in the shape core compares across
        // Providers.
        process: process.incarnation(),
        telemetry_prepared,
        telemetry_reason,
        notes: prepared
            .notes
            .into_iter()
            .chain(prepared.daemon_notes)
            .collect(),
    })
}

/// The Actor to attach to: the one given, the Actor whose current Session already holds this
/// Runtime, or under `--self` a name made from the workdir with the first free suffix.
async fn actor_name(
    ctx: &Context,
    daemon: &Daemon,
    name: Option<String>,
    self_: bool,
    process: &PlacementProcess,
    resolved: &ProviderAttachResponse,
) -> CliResult<(String, Option<Generated>)> {
    if let Some(name) = name {
        return Ok((name, None));
    }
    if !self_ {
        return Err(rejected!("external attach requires an Actor name"));
    }
    let listing = list_actors(ctx, daemon).await?;
    if let Some(actor) = holder(&listing, process) {
        return Ok((actor.name.clone(), None));
    }
    let base = normalized_actor_base(Path::new(&resolved.workdir));
    let (index, candidate) = (1..)
        .map(|index| (index, format!("{base}-{index}")))
        .find(|(_, candidate)| is_free(&listing, candidate))
        .expect("an Actor suffix is always available");
    Ok((candidate, Some((base, index + 1))))
}

/// Sends the attach. A generated name the daemon refuses moves on: to the Actor that holds
/// this Runtime by now, or to the next suffix when another Actor took the name.
async fn commit(
    ctx: &Context,
    daemon: &Daemon,
    request: &AttachActorRequest,
    mut actor: String,
    mut generated: Option<Generated>,
    process: &PlacementProcess,
) -> CliResult<AttachResponse> {
    let mut attempts = 0;
    loop {
        let error = match daemon
            .post_json(
                &format!("{}/attach", actor_path(&ctx.workspace, &actor)),
                request,
            )
            .await
        {
            Ok(response) => return Ok(response),
            Err(error @ CliError::Operational(_)) => return Err(error),
            Err(error) => error,
        };
        let Some((base, next)) = generated.as_mut() else {
            return Err(error);
        };
        attempts += 1;
        if attempts >= ATTACH_ATTEMPTS {
            return Err(error);
        }
        let listing = list_actors(ctx, daemon).await?;
        if let Some(existing) = holder(&listing, process) {
            actor = existing.name.clone();
            generated = None;
        } else if !is_free(&listing, &actor) {
            actor = format!("{base}-{next}");
            *next += 1;
        } else {
            return Err(error);
        }
    }
}

/// What step 6 delivered: the Runtime identity, and the notes on what was left undelivered.
struct Delivered {
    identity: Option<RuntimeIdentity>,
    notes: Vec<String>,
}

/// Step 6: what the Console of this Session will show depends on the Runtime identity. Under
/// `--self` this CLI runs inside the Runtime it just attached, so its own environment names
/// that Runtime. An execution pointed at from outside is named by the Runtime process in front
/// of it, because the environment here belongs to whoever typed the command. Delivering the
/// identity right away spares the Console the wait for the first hook.
async fn deliver_identity(
    daemon: &Daemon,
    adapter: &dyn RuntimeAdapter,
    runtime: Option<ProcessSnapshot>,
    token: &str,
) -> CliResult<Delivered> {
    let outside = runtime.is_some();
    let Some(identity) = adapter
        .identity(&IdentityRequest { process: runtime })?
        .identity
    else {
        let note = if outside {
            "Runtime identity is not told by the Runtime process yet; the Console shows the transcript once a hook delivers it."
        } else {
            "Runtime identity is not in this environment; the Console shows the transcript once a hook delivers it."
        };
        return Ok(Delivered {
            identity: None,
            notes: vec![note.to_owned()],
        });
    };
    let delivered = daemon
        .call_as(Method::POST, "/v1/self/runtime", token)
        .json(&identity)
        .send()
        .await
        .context("deliver Runtime identity")
        .and_then(|reply| reply.error_for_status().context("deliver Runtime identity"));
    Ok(match delivered {
        Ok(_) => Delivered {
            identity: Some(identity),
            notes: Vec::new(),
        },
        Err(error) => Delivered {
            identity: None,
            notes: vec![format!(
                "Runtime identity was not delivered ({error:#}); the Console shows the transcript once a hook delivers it."
            )],
        },
    })
}

async fn list_actors(ctx: &Context, daemon: &Daemon) -> CliResult<ActorList> {
    daemon.get(&actors_path(&ctx.workspace)).await
}

/// The Actor whose current Session already holds the Runtime process.
fn holder<'a>(listing: &'a ActorList, process: &PlacementProcess) -> Option<&'a Actor> {
    listing.actors.iter().find(|actor| {
        actor
            .current_session
            .as_ref()
            .is_some_and(|session| session.placement.runtime_process().as_ref() == Some(process))
    })
}

/// Whether an attach can take a name: no Actor has it, or an agent Actor does whose current
/// Session, if any, has gone.
fn is_free(listing: &ActorList, name: &str) -> bool {
    listing
        .actors
        .iter()
        .find(|actor| actor.name == name)
        .is_none_or(|actor| {
            actor.kind == ActorKind::Agent
                && actor.current_session.as_ref().is_none_or(|session| {
                    session.observation.as_ref().is_some_and(|observation| {
                        matches!(
                            observation.kind,
                            ObservationKind::Gone
                                | ObservationKind::Lost
                                | ObservationKind::Mismatched
                        )
                    })
                })
        })
}

fn normalized_actor_base(workdir: &Path) -> String {
    let raw = workdir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let mut result = String::new();
    let mut separator = false;
    for character in raw.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            separator = false;
            result.push(character);
        } else {
            separator = true;
        }
    }
    if result.is_empty() {
        "agent".to_owned()
    } else {
        result
    }
}
