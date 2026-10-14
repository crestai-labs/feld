//! `feld init`.

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use anyhow::Context as _;
use feld_core::home::open_lock_file;
use feld_core::plugin::protocol::PrepareRequest;
use feld_core::plugin::{PluginKind, Plugins, Registry, not_installed};
use feld_daemon::bundled;
use feld_daemon::daemon::{self, Presence};
use serde::Serialize;

use crate::args::InitArgs;
use crate::ask;
use crate::format::daemon::web_ui_line;
use crate::output::{listed, print_json};
use crate::shell_file;
use crate::style::Style;
use crate::{CliError, CliResult, Context};

/// The two kinds of Plugin, as `feld init` heads their choices.
const PLUGIN_KINDS: [(PluginKind, &str); 2] = [
    (PluginKind::Runtime, "Runtime Adapter"),
    (PluginKind::Provider, "Execution Provider"),
];

/// The last `note:` of a `feld init` that ends ready.
const DOCTOR_NOTE: &str = "run `feld doctor` if something does not work";

/// What the list `feld init` asks a terminal to check is for, and which keys do what.
const PLUGIN_ASK: (&str, &str) = (
    "Choose the Plugins to install",
    "\u{2191}\u{2193} move \u{b7} space check \u{b7} enter confirm",
);

/// One kind of Plugin as `feld init` works through it: everything Feld ships of that
/// kind, which of them the answer checks, and whether a terminal is asked for that answer.
struct KindChoice {
    title: &'static str,
    shipped: Vec<&'static bundled::BundledPlugin>,
    checked: Vec<bool>,
    asked: bool,
}

/// What `feld init` shows a person: each step on one line once it is answered, hung on
/// the rule of the steps when the output is marked, or as a `key: value` line when it is
/// plain. The step being asked is drawn by [`ask`], which takes it off the screen again.
struct InitScreen {
    style: Style,
    /// How wide the `key:` of a plain line is, so that the values line up.
    width: usize,
}

impl InitScreen {
    fn new(style: Style) -> Self {
        let width = ["Feld home", "Plugins", "Shell", "Daemon"]
            .into_iter()
            .map(str::len)
            .chain(bundled::of_kind(PluginKind::Runtime).map(|plugin| plugin.label().len()))
            .max()
            .unwrap_or_default()
            + 1;
        Self { style, width }
    }

    fn wordmark(&self) {
        let version = concat!("v", env!("CARGO_PKG_VERSION"));
        if self.style.is_marked() {
            println!(
                "{} {}",
                self.style.accent(&format!("{} Feld", self.style.wave())),
                self.style.dim(version)
            );
            println!();
        } else {
            println!("Feld {version}");
        }
    }

    /// A step, folded into its heading and its answer.
    fn answered(&self, heading: &str, answer: &str) {
        let mark = self.style.dim(self.style.answered());
        self.step(&mark, heading, answer);
    }

    /// A step that went wrong, with what went wrong as its answer.
    fn failed(&self, heading: &str, answer: &str) {
        let mark = self.style.error(self.style.failure());
        self.step(&mark, heading, answer);
    }

    fn step(&self, mark: &str, heading: &str, answer: &str) {
        if self.style.is_marked() {
            println!("{mark} {}  {answer}", self.style.dim(heading));
        } else {
            println!(
                "{:<width$} {answer}",
                format!("{heading}:"),
                width = self.width
            );
        }
    }

    /// A line under the step above it, which the step needs read in full.
    fn detail(&self, line: &str) {
        match (self.style.is_marked(), line.is_empty()) {
            (true, true) => println!("{}", self.style.dim(self.style.rule())),
            (true, false) => println!("{}  {line}", self.style.dim(self.style.rule())),
            (false, true) => {}
            (false, false) => println!("  {line}"),
        }
    }

    /// Where the steps end: whether Feld is ready, the commands to run next in the order
    /// to run them, the Web UI of the daemon this init started, and each of `notes`, every
    /// one on a line of its own.
    fn end(
        &self,
        ready: bool,
        headline: &str,
        next: &[String],
        ui: Option<&str>,
        notes: &[String],
    ) {
        let style = &self.style;
        let (indent, next_key, ui_key, note_key) = if style.is_marked() {
            let mark = if ready {
                style.ok(style.success())
            } else {
                style.error(style.failure())
            };
            println!("{}", style.dim(style.rule()));
            println!("{} {mark} {}", style.dim(style.end()), style.bold(headline));
            (
                "   ",
                style.dim("next:"),
                style.dim("ui:  "),
                style.dim("note:"),
            )
        } else {
            println!("{headline}");
            (
                "  ",
                "next:".to_owned(),
                "ui:  ".to_owned(),
                "note:".to_owned(),
            )
        };
        for command in next {
            println!("{indent}{next_key}  {}", style.bold(command));
        }
        if let Some(ui) = ui {
            println!("{indent}{ui_key}  {}", web_ui_line(ui, style));
        }
        for note in notes {
            println!("{indent}{note_key}  {note}");
        }
    }
}

/// `feld init`: the Feld home, the shipped Plugins chosen for it, the Runtimes of the
/// chosen Runtime Adapters prepared, and the daemon started to read them.
///
/// The answer comes from `--runtime` / `--provider`, else from what a terminal checks in
/// the list, else from the shipped Plugins whose products are on `PATH`. The list shows
/// everything Feld ships, so that a Plugin whose product is elsewhere is there to be
/// chosen. Plugins the answer leaves out keep whatever is installed.
///
/// A run that fails runs the checks of `feld doctor` after it says why.
pub async fn run(ctx: &Context, args: InitArgs) -> CliResult<ExitCode> {
    match init(ctx, args).await {
        Ok(code) => Ok(code),
        Err(Stop::Refused(error)) => Err(error),
        Err(Stop::Failed(error)) => {
            let code = match error {
                Some(error) => crate::report_error(ctx.json, &error),
                None => ExitCode::from(1),
            };
            super::doctor::after_failure(&ctx.home, ctx.json).await?;
            Ok(code)
        }
    }
}

/// How a `feld init` that does not finish ends.
enum Stop {
    /// Something in this environment failed, which the checks of `feld doctor` look into.
    /// None when the screen has told it already.
    Failed(Option<CliError>),
    /// The person cancelled the choice or named a Plugin Feld does not ship, which the
    /// environment has no part in.
    Refused(CliError),
}

impl<E: Into<CliError>> From<E> for Stop {
    fn from(error: E) -> Self {
        Self::Failed(Some(error.into()))
    }
}

async fn init(ctx: &Context, args: InitArgs) -> Result<ExitCode, Stop> {
    let (home, json) = (&ctx.home, ctx.json);
    let beside = bundled::executables_directory()?;
    let screen = InitScreen::new(Style::stdout(json));
    let asks = !json && !args.yes && ask::is_available();
    let answered = !args.runtime.is_empty() || !args.provider.is_empty();
    // A re-run opens the list on the answer the last `feld init` left.
    let before = Registry::load(home, None);
    let already: HashSet<String> = installed_names(&before).into_iter().collect();
    // A Provider is what every Session runs on, and Feld ships none that works without
    // its product.
    if args.provider.is_empty()
        && before.providers().is_empty()
        && bundled::on_path_of_kind(PluginKind::Provider)
            .next()
            .is_none()
    {
        return Err(rejected!("no Execution Provider is on PATH")
            .with_next(format!(
                "install {} so that it is on PATH, and run `feld init` again",
                bundled::of_kind(PluginKind::Provider)
                    .map(|plugin| plugin.name)
                    .collect::<Vec<_>>()
                    .join(" or ")
            ))
            .into());
    }
    home.create()?;
    // The lock marks the directory as a Feld home even before a daemon starts on it.
    open_lock_file(&home.lock_path(), "daemon lock")?;
    // The end says so when the Runtime it checks for want of one on PATH is missing.
    let runtime_on_path = bundled::on_path_of_kind(PluginKind::Runtime)
        .next()
        .is_some();
    if !json {
        screen.wordmark();
        screen.answered("Feld home", &home.root().display().to_string());
    }
    let mut kinds = Vec::new();
    for (kind, title) in PLUGIN_KINDS {
        let given = match kind {
            PluginKind::Runtime => &args.runtime,
            PluginKind::Provider => &args.provider,
        };
        let shipped: Vec<_> = bundled::of_kind(kind).collect();
        let installed: Vec<_> = shipped
            .iter()
            .copied()
            .filter(|plugin| already.contains(plugin.name))
            .collect();
        let selected = if !given.is_empty() {
            named_plugins(kind, given).map_err(Stop::Refused)?
        } else if asks && !installed.is_empty() {
            installed
        } else if args.yes || asks || !answered {
            detected(kind)
        } else {
            Vec::new()
        };
        kinds.push(KindChoice {
            title,
            checked: shipped
                .iter()
                .map(|plugin| selected.iter().any(|one| one.name == plugin.name))
                .collect(),
            asked: asks && given.is_empty(),
            shipped,
        });
    }
    ask_for_plugins(&mut kinds, &screen.style).map_err(Stop::Refused)?;
    let chosen: Vec<&'static bundled::BundledPlugin> = kinds
        .iter()
        .flat_map(|one| one.shipped.iter().zip(&one.checked))
        .filter(|(_, checked)| **checked)
        .map(|(plugin, _)| *plugin)
        .collect();
    for plugin in &chosen {
        let executable = plugin.executable(&beside);
        if !executable.is_file() {
            return Err(rejected!(
                "the executable of the {} Plugin is not beside feld: {}",
                plugin.name,
                executable.display()
            )
            .with_next(bundled::reinstall_next(&[plugin.name]))
            .into());
        }
    }
    for plugin in &chosen {
        plugin.install(home, &beside)?;
    }
    let installed = Registry::load(home, None);
    let selected: HashSet<&str> = chosen.iter().map(|plugin| plugin.name).collect();
    let mut notes: Vec<String> = chosen
        .iter()
        .filter(|plugin| {
            plugin.kind == PluginKind::Runtime && args.runtime.is_empty() && !runtime_on_path
        })
        .map(|plugin| {
            format!(
                "{}: `{}` is not on PATH; install {} to run it",
                plugin.label(),
                plugin.command,
                plugin.label()
            )
        })
        .collect();
    let warnings: Vec<String> = installed
        .runtimes()
        .iter()
        .map(|plugin| (PluginKind::Runtime, plugin))
        .chain(
            installed
                .providers()
                .iter()
                .map(|plugin| (PluginKind::Provider, plugin)),
        )
        .filter_map(|(kind, installed)| {
            let plugin = bundled::find(kind, &installed.name)?;
            let line = super::doctor::version_line(Path::new(plugin.command), plugin.version_args)?;
            plugin.unsupported(&line)
        })
        .collect();
    if !json {
        let style = &screen.style;
        let kind = |key: &str, plugins: &[feld_core::plugin::PluginSummary]| {
            let names: Vec<String> = plugins
                .iter()
                .map(|plugin| {
                    if selected.contains(plugin.name.as_str()) {
                        plugin.name.clone()
                    } else {
                        format!("{} {}", plugin.name, style.warn("(kept)"))
                    }
                })
                .collect();
            format!("{} {}", style.dim(key), listed(names))
        };
        screen.answered(
            "Plugins",
            &format!(
                "{}  {}",
                kind("runtimes:", &installed.runtimes()),
                kind("providers:", &installed.providers())
            ),
        );
        for warning in &warnings {
            screen.detail(&format!("{} {warning}", style.warn("warning:")));
        }
    }
    let preparations = prepare_runtimes(
        &installed,
        &chosen,
        args.yes,
        asks,
        (!json).then_some(&screen),
    )?;
    let kept: Vec<String> = installed_names(&installed)
        .into_iter()
        .filter(|name| !selected.contains(name.as_str()))
        .collect();
    // A kind left with nothing installed takes its default before the daemon.
    let missing: Vec<PluginKind> = [
        (PluginKind::Runtime, installed.runtimes()),
        (PluginKind::Provider, installed.providers()),
    ]
    .into_iter()
    .filter(|(_, plugins)| plugins.is_empty())
    .map(|(kind, _)| kind)
    .collect();
    let pending = preparations
        .iter()
        .any(|one| matches!(one.outcome, Preparation::NeedsConsent(_)));
    // A shell connects to Feld once Feld is ready, so a run that ends before that leaves
    // the shell settings for the next.
    let shell = if pending || !missing.is_empty() {
        None
    } else {
        Some(add_feld_env(args.yes, asks, (!json).then_some(&screen))?)
    };
    notes.extend(shell.as_ref().and_then(ShellSetup::note));
    // The daemon reads the Plugin directories when it starts, so a running one takes them
    // on a restart.
    let running_status = match daemon::probe(home).await {
        Ok(Presence::Running(status)) => Some(status),
        _ => None,
    };
    let running = running_status.is_some();
    let daemon_next = if running {
        "feld daemon restart"
    } else {
        "feld daemon start"
    };
    if json {
        let next = if missing.is_empty() {
            daemon_next.to_owned()
        } else {
            feld_core::plugin::default_init(&missing)
        };
        print_json(&serde_json::json!({
            "home": home.root(),
            "chosen": chosen.iter().map(|plugin| plugin.name).collect::<Vec<_>>(),
            "kept": kept,
            "runtimes": installed.runtimes(),
            "providers": installed.providers(),
            "next": next,
            "notes": notes,
            "warnings": warnings,
            "shell": shell,
            "ui": running_status.and_then(|status| status.web_ui),
            "prepared": preparations
                .iter()
                .map(|one| (one.name, &one.outcome))
                .collect::<std::collections::BTreeMap<_, _>>(),
        }))?;
        return Ok(if pending {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        });
    }
    if pending {
        screen.end(false, "Feld is not ready.", &[], None, &[]);
        return Ok(ExitCode::from(1));
    }
    if !missing.is_empty() {
        screen.end(
            false,
            "Feld is not ready.",
            &[feld_core::plugin::default_init(&missing)],
            None,
            &[],
        );
        return Ok(ExitCode::SUCCESS);
    }
    let start = if args.yes {
        Some(true)
    } else if asks {
        let style = &screen.style;
        let (question, body) = if running {
            (
                "Restart the daemon to load the Plugins?",
                vec![style.dim("Sessions keep running on their providers.")],
            )
        } else {
            ("Start the daemon now?", Vec::new())
        };
        Some(ask::yes_or_no(
            &style.bold("Daemon"),
            &body,
            question,
            style,
        )?)
    } else {
        None
    };
    let mut next = Vec::new();
    let mut ui = None;
    match start {
        Some(true) => {
            let started = if running {
                daemon::restart(home, None, &[])
                    .await
                    .map(|(_, status)| status)
            } else {
                daemon::start(home, None, &[])
                    .await
                    .map(|(status, _)| status)
            };
            match started {
                Ok(status) => {
                    let verb = if running { "restarted" } else { "started" };
                    screen.answered(
                        "Daemon",
                        &format!("{verb} \u{b7} pid {} \u{b7} {}", status.pid, status.api),
                    );
                    next.push("feld ui".to_owned());
                    ui = status.web_ui;
                }
                Err(error) => {
                    screen.failed("Daemon", &format!("{error:#}"));
                    screen.detail("run `feld daemon start` to start it once the cause is fixed");
                    screen.end(false, "Feld is not ready.", &[], None, &[]);
                    return Err(Stop::Failed(None));
                }
            }
        }
        Some(false) => {
            let answer = if running {
                "not restarted"
            } else {
                "not started"
            };
            screen.answered("Daemon", &screen.style.dim(answer));
            next.push(daemon_next.to_owned());
        }
        None => next.push(daemon_next.to_owned()),
    }
    // A shell that has not read the line yet takes it before the commands that follow.
    if let Some(path) = shell
        .as_ref()
        .and_then(ShellSetup::reading)
        .filter(|_| !super::env::connected(home))
    {
        next.insert(
            0,
            format!(
                "source {}",
                feld_core::process::quote_for_display(&path.display().to_string())
            ),
        );
    }
    // A daemon this run started takes the environment of this shell, which settles what the
    // Runtimes noted about the one running before.
    let started = start == Some(true);
    notes.extend(preparations.iter().flat_map(|one| {
        one.outcome
            .response()
            .into_iter()
            .flat_map(|response| {
                let daemon_notes = if started {
                    &[][..]
                } else {
                    &response.daemon_notes[..]
                };
                response.notes.iter().chain(daemon_notes)
            })
            .map(|note| format!("{}: {note}", one.label))
    }));
    notes.push(DOCTOR_NOTE.to_owned());
    screen.end(true, "Feld is ready.", &next, ui.as_deref(), &notes);
    Ok(ExitCode::SUCCESS)
}

/// How `feld init` left the shell settings file that connects a shell to Feld.
#[derive(Serialize)]
struct ShellSetup {
    /// The file the line is added to, or the one that already has it; None for a shell
    /// that is neither zsh nor bash.
    path: Option<std::path::PathBuf>,
    status: ShellStatus,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ShellStatus {
    Added,
    Present,
    Declined,
    NeedsConsent,
    Unsupported,
}

impl ShellSetup {
    /// The file that reads `feld env` now.
    fn reading(&self) -> Option<&std::path::Path> {
        matches!(self.status, ShellStatus::Added | ShellStatus::Present)
            .then_some(self.path.as_deref())
            .flatten()
    }

    /// What a person is to do about a line `feld init` did not add.
    fn note(&self) -> Option<String> {
        let line = shell_file::FELD_ENV_LINE;
        match (self.status, &self.path) {
            (ShellStatus::Added | ShellStatus::Present, _) => None,
            (_, Some(path)) => Some(format!(
                "add `{line}` to {} to connect your shell to Feld",
                path.display()
            )),
            (_, None) => Some(format!(
                "add `{line}` to the settings file of your shell to connect it to Feld"
            )),
        }
    }
}

/// Adds the `feld env` line to the file an interactive shell reads, with consent:
/// `--yes`, or the terminal's answer. A file that reads `feld env` already keeps as it is.
/// This is one step of the screen, if there is one.
fn add_feld_env(yes: bool, asks: bool, screen: Option<&InitScreen>) -> CliResult<ShellSetup> {
    let line = shell_file::FELD_ENV_LINE;
    let style = screen.map_or_else(Style::plain, |screen| screen.style);
    let (setup, answer) = match shell_file::shell() {
        Err(shell) => (
            ShellSetup {
                path: None,
                status: ShellStatus::Unsupported,
            },
            style.warn(&format!("not added ({shell} is not zsh or bash)")),
        ),
        Ok(shell) => match shell.reading_feld_env() {
            Some(path) => (
                ShellSetup {
                    path: Some(path.to_path_buf()),
                    status: ShellStatus::Present,
                },
                format!("{line} already in {}", path.display()),
            ),
            None => {
                let path = shell.interactive;
                let consented = if yes {
                    true
                } else if asks {
                    let heading = format!(
                        "{}  {}",
                        style.bold("Shell"),
                        style.dim(&path.display().to_string())
                    );
                    let body = [shell_file::FELD_ENV_COMMENT, line]
                        .map(|added| style.ok(&format!("+ {added}")));
                    ask::yes_or_no(
                        &heading,
                        &body,
                        "Add this line to your shell settings?",
                        &style,
                    )?
                } else {
                    false
                };
                let (status, answer) = if consented {
                    shell_file::add_feld_env(&path)?;
                    (
                        ShellStatus::Added,
                        format!("added {line} to {}", path.display()),
                    )
                } else if asks {
                    (ShellStatus::Declined, style.warn("not added (declined)"))
                } else {
                    (
                        ShellStatus::NeedsConsent,
                        style.warn("not added (needs your consent)"),
                    )
                };
                (
                    ShellSetup {
                        path: Some(path),
                        status,
                    },
                    answer,
                )
            }
        },
    };
    if let Some(screen) = screen {
        screen.answered("Shell", &answer);
    }
    Ok(setup)
}

/// One Runtime Adapter `feld init` prepared the Runtime of, and how that went.
struct RuntimePreparation {
    name: &'static str,
    label: String,
    outcome: Preparation,
}

/// How preparing one Runtime ended.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum Preparation {
    /// The entries are written, or were Feld's already.
    Prepared(feld_core::plugin::protocol::PrepareResponse),
    /// Some entries replace the person's own values, and nobody consented; nothing was
    /// written.
    NeedsConsent(feld_core::plugin::protocol::PrepareResponse),
    /// The terminal declined; nothing was written.
    Declined(feld_core::plugin::protocol::PrepareResponse),
}

impl Preparation {
    fn response(&self) -> Option<&feld_core::plugin::protocol::PrepareResponse> {
        match self {
            Self::Prepared(response) => Some(response),
            Self::NeedsConsent(_) | Self::Declined(_) => None,
        }
    }
}

/// Prepares the Runtime of every Runtime Adapter chosen this run for a person to start and
/// attach. An entry that chains or replaces a value of the person's own is written only
/// with consent: `--yes`, or the terminal's answer to the entries it is shown. Each Runtime
/// is one step of the screen, if there is one.
fn prepare_runtimes(
    installed: &Registry,
    chosen: &[&'static bundled::BundledPlugin],
    yes: bool,
    asks: bool,
    screen: Option<&InitScreen>,
) -> CliResult<Vec<RuntimePreparation>> {
    use feld_core::plugin::protocol::PrepareAction;

    let mut preparations = Vec::new();
    for plugin in chosen
        .iter()
        .filter(|plugin| plugin.kind == PluginKind::Runtime)
    {
        let adapter = installed
            .runtime(plugin.name)
            .ok_or_else(|| CliError::Rejected(not_installed(PluginKind::Runtime, plugin.name)))?;
        let checked = adapter.prepare(&PrepareRequest {
            apply: false,
            collector: None,
        })?;
        let label = plugin.label();
        let style = screen.map_or_else(Style::plain, |screen| screen.style);
        let changes = changed_entries(&checked, &style);
        let needs_consent = checked
            .entries
            .iter()
            .any(|entry| entry.action.needs_consent());
        let outcome = if !needs_consent || yes {
            None
        } else if asks {
            // The settings the consent is for head the entries, so that a person can check
            // where they are written.
            let settings = checked
                .entries
                .iter()
                .find(|entry| entry.action.needs_consent())
                .map(|entry| entry.path.as_str())
                .unwrap_or_default();
            let heading = format!("{}  {}", style.bold(&label), style.dim(settings));
            let question = format!("Apply these changes to your {label} settings?");
            (!ask::yes_or_no(&heading, &changes, &question, &style)?)
                .then(|| Preparation::Declined(checked.clone()))
        } else {
            Some(Preparation::NeedsConsent(checked.clone()))
        };
        let outcome = match outcome {
            Some(outcome) => outcome,
            None => Preparation::Prepared(adapter.prepare(&PrepareRequest {
                apply: true,
                collector: None,
            })?),
        };
        if let Some(screen) = screen {
            match &outcome {
                Preparation::Prepared(_) => {
                    let mut answer = vec!["prepared".to_owned()];
                    for entry in &checked.entries {
                        match entry.action {
                            PrepareAction::Chain => answer.push(format!("{} chained", entry.key)),
                            PrepareAction::Replace => {
                                answer.push(format!("{} replaced", entry.key));
                            }
                            PrepareAction::Add => answer.push(format!("{} added", entry.key)),
                            PrepareAction::Write | PrepareAction::Keep => {}
                        }
                    }
                    screen.answered(&label, &answer.join(" \u{b7} "));
                }
                Preparation::Declined(_) => {
                    screen.answered(&label, &style.warn("not prepared (declined)"));
                }
                Preparation::NeedsConsent(_) => {
                    screen.failed(&label, "not prepared (needs your consent)");
                    for line in &changes {
                        screen.detail(line);
                    }
                    screen.detail("");
                    screen.detail("run `feld init --yes` to apply them");
                }
            }
        }
        preparations.push(RuntimePreparation {
            name: plugin.name,
            label,
            outcome,
        });
    }
    Ok(preparations)
}

/// The entries `prepare` would change, as a person reads them: the key, then the value it
/// has now (`-`) and the value Feld writes (`+`). A key whose value Feld keeps says where
/// the value goes. A blank line parts the entries.
fn changed_entries(
    response: &feld_core::plugin::protocol::PrepareResponse,
    style: &Style,
) -> Vec<String> {
    use feld_core::plugin::protocol::PrepareAction;

    let mut lines = Vec::new();
    for entry in response
        .entries
        .iter()
        .filter(|entry| entry.action != PrepareAction::Keep)
    {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let kept = match entry.action {
            PrepareAction::Chain => {
                Some("the current command keeps running, chained behind Feld's")
            }
            PrepareAction::Replace if entry.key.ends_with('/') => {
                Some("files of other names stay; Feld's are written over those of its names")
            }
            PrepareAction::Replace => Some("the current value is kept in Feld's prepare record"),
            PrepareAction::Add => {
                Some("the values there stay; Feld's joins them and widens what the Runtime may do")
            }
            PrepareAction::Write | PrepareAction::Keep => None,
        };
        lines.push(match kept {
            Some(kept) => format!("{}  {}", entry.key, style.dim(kept)),
            None => entry.key.clone(),
        });
        let shown_current = entry
            .current
            .as_ref()
            .filter(|_| entry.action.needs_consent());
        if let Some(current) = shown_current {
            lines.push(style.error(&format!("- {current}")));
        }
        // A replaced value with nothing after it leaves the key out.
        match (&entry.next, shown_current) {
            (Some(next), _) => lines.push(style.ok(&format!("+ {next}"))),
            (None, None) => lines.push(style.ok(&format!("+ {}", entry.path))),
            (None, Some(_)) => {}
        }
    }
    lines
}

/// The shipped Plugins of one kind whose products are on `PATH`. With no Runtime there,
/// the default Runtime stands in, so that a person who installs it later finds it ready.
fn detected(kind: PluginKind) -> Vec<&'static bundled::BundledPlugin> {
    let found: Vec<_> = bundled::on_path_of_kind(kind).collect();
    if found.is_empty() && kind == PluginKind::Runtime {
        bundled::default_of_kind(kind).collect()
    } else {
        found
    }
}

/// The names of the installed Plugins, both kinds together, Runtimes first.
fn installed_names(installed: &Registry) -> Vec<String> {
    installed
        .runtimes()
        .into_iter()
        .chain(installed.providers())
        .map(|plugin| plugin.name)
        .collect()
}

/// The shipped Plugins of one kind that a list of names asks for.
fn named_plugins(
    kind: PluginKind,
    names: &[String],
) -> CliResult<Vec<&'static bundled::BundledPlugin>> {
    let mut chosen: Vec<&'static bundled::BundledPlugin> = Vec::new();
    for name in names {
        let plugin = bundled::find(kind, name).ok_or_else(|| {
            rejected!(
                "Feld ships no {} Plugin named '{name}' (it ships: {})",
                kind.as_str(),
                bundled::of_kind(kind)
                    .map(|plugin| plugin.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if !chosen.iter().any(|one| one.name == plugin.name) {
            chosen.push(plugin);
        }
    }
    Ok(chosen)
}

/// Asks a terminal which Plugins to install: one list, headed kind by kind, whose checks
/// become the answer of the kinds no flag answered.
fn ask_for_plugins(kinds: &mut [KindChoice], style: &Style) -> CliResult<()> {
    let asked: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, one)| one.asked)
        .map(|(at, _)| at)
        .collect();
    if asked.is_empty() {
        return Ok(());
    }
    let mut groups: Vec<ask::Group> = asked
        .iter()
        .map(|at| {
            let one = &kinds[*at];
            ask::Group {
                title: one.title.to_owned(),
                choices: one
                    .shipped
                    .iter()
                    .zip(&one.checked)
                    .map(|(plugin, checked)| ask::Choice {
                        name: plugin.name.to_owned(),
                        label: plugin.label(),
                        checked: *checked,
                    })
                    .collect(),
            }
        })
        .collect();
    match ask::check(&mut groups, PLUGIN_ASK.0, PLUGIN_ASK.1, style)
        .context("ask which Plugins to install")?
    {
        ask::Answer::Confirmed => {}
        ask::Answer::Left => {
            return Err(rejected!(
                "the choice was cancelled; no Plugin was installed"
            ));
        }
    }
    for (at, group) in asked.into_iter().zip(groups) {
        kinds[at].checked = group.choices.iter().map(|choice| choice.checked).collect();
    }
    Ok(())
}
