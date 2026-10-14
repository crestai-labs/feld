//! What a fixture Feld home needs before anything reads it: the Plugins Feld ships,
//! installed the way a person installs them, the stand-in Runtimes the E2E tests drive,
//! and the herdr server the herdr tests drive.
//!
//! Every Runtime Adapter and Execution Provider is a Plugin directory now, so a home
//! without them has neither. Tests fill one by running `feld init`, which is what fills a
//! real home, so no test can drift from what a person gets.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Once;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use feld_core::telemetry::proto;
use feld_core::test_require;
use prost::Message;
use serde_json::Value;

pub mod stand_in;

const FELD: &str = env!("CARGO_BIN_EXE_feld");

/// The executable of a Plugin Feld ships, which `cargo build` puts beside `feld`. It is
/// the binary of another crate of the workspace, which `cargo test` leaves as the last
/// `cargo build` made it, so the first Plugin a test process asks for builds the
/// workspace's binaries from the sources this test was built from.
pub fn plugin_executable(name: &str) -> PathBuf {
    static BUILT: Once = Once::new();
    BUILT.call_once(build_workspace_binaries);
    let executable = Path::new(FELD).with_file_name(format!("feld-plugin-{name}"));
    assert!(
        executable.is_file(),
        "{} is not beside feld",
        executable.display()
    );
    executable
}

/// Builds every binary of the workspace into the directory and profile `feld` came from.
fn build_workspace_binaries() {
    let profile_dir = Path::new(FELD).parent().unwrap();
    let profile = match profile_dir.file_name().unwrap().to_str().unwrap() {
        "debug" => "dev",
        other => other,
    };
    let mut command = Command::new(env!("CARGO"));
    // Cargo gives a test the variables of its package, which build scripts such as ring's
    // watch, so a build that saw them would differ from `cargo build` and rebuild each time.
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if [
            "CARGO_MANIFEST_",
            "CARGO_PKG_",
            "CARGO_CRATE_",
            "CARGO_BIN_",
            "CARGO_PRIMARY_PACKAGE",
            "CARGO_TARGET_TMPDIR",
            "OUT_DIR",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        {
            command.env_remove(&*name);
        }
    }
    let output = command
        .args(["build", "--workspace", "--bins", "--profile", profile])
        .arg("--target-dir")
        .arg(profile_dir.parent().unwrap())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cargo build");
    assert!(
        output.status.success(),
        "cargo build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `feld` as a fixture runs it: nothing of the Feld Session, Runtime, or pane this may run
/// inside reaches it, so it answers from the fixture home and its own daemon alone.
pub fn feld() -> Command {
    let mut command = Command::new(FELD);
    without_outside_variables(&mut command);
    command
}

/// Ends the parts a Runtime Adapter of `home` runs beside a Runtime. They outlive the
/// Runtime that started them by design, so a test that drove a stand-in Runtime ends them
/// with the rest of its fixture.
pub fn stop_runtime_helpers(home: &Path) {
    let _ = Command::new("pkill")
        .arg("-f")
        .arg(home.join("plugins/runtime/"))
        .status();
}

/// The variables that tell a program about the Feld Session, the Runtime, and the tmux or
/// herdr pane the test process may run inside. Through them a test would reach the daemon,
/// the configuration root (`~/.claude`, `~/.codex`), and the tmux server of the person
/// running it.
const OUTSIDE_VARIABLES: &[&str] = &[
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "CODEX_THREAD_ID",
    "CODEX_SESSION_ID",
    "TMUX",
    "TMUX_PANE",
];

/// Takes every `FELD_*` and `HERDR_*` variable and each of `OUTSIDE_VARIABLES` out of what
/// `command` inherits. A variable the fixture sets afterwards still reaches it.
pub fn without_outside_variables(command: &mut Command) -> &mut Command {
    for (variable, _) in std::env::vars_os() {
        let name = variable.to_string_lossy();
        if name.starts_with("FELD_") || name.starts_with("HERDR_") {
            command.env_remove(&*name);
        }
    }
    for variable in OUTSIDE_VARIABLES {
        command.env_remove(variable);
    }
    command
}

/// Installs the Plugins Feld ships into a fixture `$FELD_HOME`.
///
/// The defaults are named rather than taken with `--yes`, which starts a daemon as well, so
/// that each test starts its daemon itself.
pub fn install_plugins(home: &Path) {
    init(
        home,
        &["init", "--runtime", "claude-code", "--provider", "tmux"],
    );
}

/// Installs the Plugins Feld ships and the codex Runtime Adapter beside them.
///
/// codex sits outside the default choice, the way herdr does, so only the tests that put a
/// Session on Codex ask for it.
pub fn install_plugins_with_codex(home: &Path) {
    // Adding the spool to the roots Codex's sandbox may write takes consent, which `--yes`
    // would give to the shell settings as well; the roots already name it instead.
    let codex = home.join(".codex");
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        codex.join("config.toml"),
        format!(
            "[sandbox_workspace_write]\nwritable_roots = [\"{}\"]\n",
            fs::canonicalize(home)
                .unwrap()
                .join("spool/shared")
                .display()
        ),
    )
    .unwrap();
    init(
        home,
        &[
            "init",
            "--runtime",
            "claude-code",
            "--runtime",
            "codex",
            "--provider",
            "tmux",
        ],
    );
}

/// Installs the Plugins Feld ships and the herdr Provider beside them.
///
/// herdr sits outside the default choice, so only the tests that place a Session on it ask
/// for it and every other fixture home resolves a Provider the way a person's home does.
pub fn install_plugins_with_herdr(home: &Path) {
    init(
        home,
        &[
            "init",
            "--runtime",
            "claude-code",
            "--provider",
            "tmux",
            "--provider",
            "herdr",
        ],
    );
}

fn init(home: &Path, arguments: &[&str]) {
    for pair in arguments.windows(2) {
        if matches!(pair[0], "--runtime" | "--provider") {
            plugin_executable(pair[1]);
        }
    }
    let output = feld()
        .arg("--home")
        .arg(home)
        .args(arguments)
        // `feld init` prepares the Runtimes' configuration roots, which are under HOME here.
        .env("HOME", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .output()
        .expect("run feld init");
    assert!(
        output.status.success(),
        "feld init failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    warm_up_plugins(home);
    for root in [".claude", ".codex"] {
        if home.join(root).join(RUNTIME_PARTS).is_dir() {
            warm_up_runtime_parts(&home.join(root));
        }
    }
}

/// Runs each executable of the Plugins installed in `home` once. macOS inspects an
/// executable the first time it runs, which takes seconds while the machine builds and runs
/// others, and `feld init` installs new files each time. Warming them up keeps that
/// inspection out of what a test waits for.
///
/// The Plugin executable runs without an operation. A part in `bin/` runs through a link
/// beside no Plugin executable, so it ends without reaching one.
pub fn warm_up_plugins(home: &Path) {
    let apart = tempfile::tempdir_in(home).unwrap();
    let apart = apart.path().join("bin");
    fs::create_dir(&apart).unwrap();
    for kind in fs::read_dir(home.join("plugins")).unwrap() {
        for plugin in fs::read_dir(kind.unwrap().path()).unwrap() {
            let plugin = plugin.unwrap().path();
            warm_up(
                without_outside_variables(&mut Command::new(plugin.join("plugin")))
                    .env("HOME", home),
            );
            for part in fs::read_dir(plugin.join("bin")).into_iter().flatten() {
                let part = part.unwrap().path();
                let link = apart.join(part.file_name().unwrap());
                fs::hard_link(&part, &link).unwrap();
                warm_up(without_outside_variables(&mut Command::new(&link)).env("HOME", home));
                fs::remove_file(&link).unwrap();
            }
        }
    }
}

/// Where `feld init` writes the parts a Runtime runs, under its configuration root.
const RUNTIME_PARTS: &str = "skills/feld/bin";

/// Runs each part `feld init` wrote into the Runtime configuration `root` once, as
/// `warm_up_plugins` does. A part there finds its Plugin through `FELD_HOME`, so it runs
/// against an empty one and ends without reaching the Plugin. The monitor waits there in
/// `sleep`, so the `PATH` is empty too and the part ends where it would wait.
pub fn warm_up_runtime_parts(root: &Path) {
    let empty = tempfile::tempdir().unwrap();
    for part in fs::read_dir(root.join(RUNTIME_PARTS)).unwrap() {
        warm_up(
            Command::new(part.unwrap().path())
                .env("FELD_HOME", empty.path())
                .env("HOME", empty.path())
                .env("PATH", empty.path()),
        );
    }
}

/// What a stand-in program adds to its source so that a warm-up run ends before `main`.
const WARM_UP_SOURCE: &str = "#include <stdlib.h>\n#include <unistd.h>\n__attribute__((constructor)) static void feld_test_warm_up(void) { if (getenv(\"FELD_TEST_WARM_UP\")) _exit(0); }\n";

/// Compiles the C `source` of a stand-in program to `path` and warms it up the way
/// `warm_up_plugins` does. Answers whether this machine has a C compiler to build one with.
pub fn compile_program(path: &Path, source: &str) -> bool {
    let file = path.with_extension("c");
    fs::write(&file, format!("{source}{WARM_UP_SOURCE}")).unwrap();
    let compiled = Command::new("cc")
        .arg(&file)
        .arg("-o")
        .arg(path)
        .status()
        .is_ok_and(|status| status.success());
    if compiled {
        warm_up(Command::new(path).env("FELD_TEST_WARM_UP", "1"));
    } else {
        // The stand-ins are what the tmux tests run in a pane.
        test_require::step_aside("tmux", "no C compiler to build a stand-in with");
    }
    compiled
}

/// Runs `command` once and waits for it, so that macOS has inspected the executable before
/// a test waits on it. See [`warm_up_plugins`].
pub fn warm_up(command: &mut Command) {
    let _ = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// A stand-in for a Runtime: a program that optionally runs one command and then waits,
/// so that a test can attach to it, read it, and end it. The file name is the Runtime the
/// Adapter recognises, so a test names it `claude` or `codex`. Answers whether this machine
/// has a C compiler to build one with.
pub fn compile_runtime(path: &Path, child_command: Option<&str>) -> bool {
    let child = child_command
        .map(|command| {
            format!(
                "if (system(\"{}\") != 0) return 2;",
                command.replace('\\', "\\\\").replace('"', "\\\"")
            )
        })
        .unwrap_or_default();
    compile_program(
        path,
        &format!(
            "#include <stdlib.h>\n#include <unistd.h>\nint main(void) {{ {child} sleep(300); return 0; }}\n"
        ),
    )
}

/// One of the parts the claude-code Plugin runs beside Claude Code, reached through the
/// shim in the installed Plugin directory the way Claude Code reaches it.
pub fn runtime_helper(home: &Path, name: &str) -> Command {
    let mut command = Command::new(home.join("plugins/runtime/claude-code/bin").join(name));
    without_outside_variables(&mut command)
        .env("FELD_HOME", home)
        .env("HOME", home)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE");
    command
}

/// One of the two parts the codex Plugin runs beside Codex, reached through the shim in
/// the installed Plugin directory the way Codex reaches it.
pub fn codex_runtime_helper(home: &Path, name: &str) -> Command {
    let mut command = Command::new(home.join("plugins/runtime/codex/bin").join(name));
    without_outside_variables(&mut command)
        .env("FELD_HOME", home)
        .env("HOME", home)
        .env_remove("CODEX_HOME")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE");
    command
}

// --- The stand-in `codex` ------------------------------------------------------------

/// The day directory the stand-in Codex writes its rollout files in, under the
/// configuration root it reads from its own environment.
const ROLLOUT_DAY: &str = "sessions/2026/09/21";
/// What Codex names a rollout file before the thread id: `rollout-<start>-`.
const ROLLOUT_PREFIX: &str = "rollout-2026-09-21T09-28-48-";

/// A stand-in for `codex`: one program in both the roles a Codex plays for Feld.
///
/// Started as the Runtime it records the argv and environment Feld launched it with,
/// writes the rollout files of its thread and of the Sub Agent it spawned where Codex
/// writes them, runs whatever a Codex runs beside itself, and ends on the `/quit` a
/// `feld session stop` types into its pane. Called as `codex queue …` it records the call and
/// returns, which is how the queue helper reaches Codex.
#[derive(Default)]
pub struct FakeCodex {
    record: Option<PathBuf>,
    calls: Option<PathBuf>,
    refusal: Option<PathBuf>,
    taken: Option<PathBuf>,
    rollouts: Vec<(PathBuf, String)>,
    held: Option<PathBuf>,
    child: Option<String>,
}

impl FakeCodex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes the launch down: the argv, a blank line, then the environment.
    pub fn recording(mut self, record: &Path) -> Self {
        self.record = Some(record.to_owned());
        self
    }

    /// Answers `codex queue …` by writing the call down, one argument per line.
    pub fn queueing(mut self, calls: &Path) -> Self {
        self.calls = Some(calls.to_owned());
        self
    }

    /// Refuses a `codex queue` while this file is there, the way Codex refuses a Message
    /// it has no rollout file to put in the thread's queue yet.
    pub fn refusing_while(mut self, marker: &Path) -> Self {
        self.refusal = Some(marker.to_owned());
        self
    }

    /// Takes each queued Message into a turn at once, writing it into `rollout` as the
    /// `user` message Codex writes when a turn takes a queued input.
    pub fn taking_into(mut self, rollout: &Path) -> Self {
        self.taken = Some(rollout.to_owned());
        self
    }

    /// Writes one of the captured rollout files under `$CODEX_HOME`, named after the
    /// thread it belongs to, the way Codex writes one on its first input.
    pub fn with_rollout(mut self, fixture: &str, thread: &str) -> Self {
        self.rollouts
            .push((fixture_rollout(fixture), thread.to_owned()));
        self
    }

    /// Holds `rollout` open while it runs, the way Codex holds the rollout file of its thread.
    pub fn holding_open(mut self, rollout: &Path) -> Self {
        self.held = Some(rollout.to_owned());
        self
    }

    /// What Codex runs beside itself once it has started, through the shell.
    pub fn beside(mut self, child: &str) -> Self {
        self.child = Some(child.to_owned());
        self
    }

    /// Compiles it at `path`, whose file name is the `codex` an Adapter and a `PATH`
    /// lookup both reach. Answers whether this machine has a C compiler to build one with.
    pub fn compile(&self, path: &Path) -> bool {
        compile_program(path, &self.source())
    }

    fn source(&self) -> String {
        let mut body = String::new();
        if let Some(calls) = &self.calls {
            let refusal = match &self.refusal {
                Some(marker) => format!(
                    "if (access({}, F_OK) == 0) {{ fprintf(stderr, \"no rollout for this thread yet\\n\"); return 1; }}",
                    literal(&marker.display().to_string())
                ),
                None => String::new(),
            };
            let taking = match &self.taken {
                Some(rollout) => format!(
                    "put({}, cat(cat(\"{{\\\"type\\\":\\\"response_item\\\",\\\"payload\\\":{{\\\"type\\\":\\\"message\\\",\\\"role\\\":\\\"user\\\",\\\"content\\\":[{{\\\"type\\\":\\\"input_text\\\",\\\"text\\\":\\\"\", escaped(argv[argc - 1])), \"\\\"}}]}}}}\\n\"), 0);",
                    literal(&rollout.display().to_string())
                ),
                None => String::new(),
            };
            body.push_str(&format!(
                "  if (argc > 1 && strcmp(argv[1], \"queue\") == 0) {{\n    put({}, cat(lines(argv + 1, argc - 1), \"\\n\"), 0);\n    {refusal}\n    {taking}\n    return 0;\n  }}\n",
                literal(&calls.display().to_string())
            ));
        }
        if let Some(record) = &self.record {
            body.push_str(&format!(
                "  put({}, cat(cat(lines(argv, argc), \"\\n\"), lines(environ, -1)), 1);\n",
                literal(&record.display().to_string())
            ));
        }
        for (fixture, thread) in &self.rollouts {
            // The configuration root is Codex's own, read where Codex reads it, so a test
            // says where the rollout files go by saying what `CODEX_HOME` is.
            let shell = format!(
                "root=\"${{CODEX_HOME:-$HOME/.codex}}\"; mkdir -p \"$root/{ROLLOUT_DAY}\" && cp '{}' \"$root/{ROLLOUT_DAY}/{ROLLOUT_PREFIX}{thread}.jsonl\"",
                fixture.display()
            );
            body.push_str(&format!(
                "  if (system({}) != 0) return 3;\n",
                literal(&shell)
            ));
        }
        if let Some(held) = &self.held {
            body.push_str(&format!(
                "  if (open({}, O_RDONLY) < 0) return 4;\n",
                literal(&held.display().to_string())
            ));
        }
        if let Some(child) = &self.child {
            body.push_str(&format!(
                "  if (system({}) != 0) return 2;\n",
                literal(child)
            ));
        }
        format!(
            "{CODEX_SOURCE_PREAMBLE}int main(int argc, char **argv) {{\n{body}{CODEX_SOURCE_WAIT}}}\n"
        )
    }
}

/// The parts of the stand-in that never change: writing a record with one append, so that
/// two calls at once stay two records, and joining a list of strings into lines.
const CODEX_SOURCE_PREAMBLE: &str = r#"#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
extern char **environ;

/* Every value on its own line. A count below zero reads a NULL-terminated list. */
static char *lines(char **values, int count) {
  size_t length = 1;
  int i;
  for (i = 0; count < 0 ? values[i] != 0 : i < count; i++) length += strlen(values[i]) + 1;
  char *text = malloc(length);
  text[0] = 0;
  for (i = 0; count < 0 ? values[i] != 0 : i < count; i++) {
    strcat(text, values[i]);
    strcat(text, "\n");
  }
  return text;
}

static char *cat(const char *left, const char *right) {
  char *text = malloc(strlen(left) + strlen(right) + 1);
  text[0] = 0;
  strcat(text, left);
  strcat(text, right);
  return text;
}

/* A string as the body of a JSON string. */
static char *escaped(const char *text) {
  char *out = malloc(strlen(text) * 2 + 1);
  char *at = out;
  for (; *text; text++) {
    if (*text == '"' || *text == '\\') *at++ = '\\';
    *at++ = *text;
  }
  *at = 0;
  return out;
}

/* One write, so that a reader sees a whole record or none of it. */
static void put(const char *path, const char *text, int truncate) {
  int flags = O_WRONLY | O_CREAT | (truncate ? O_TRUNC : O_APPEND);
  int fd = open(path, flags, 0600);
  if (fd < 0) return;
  if (write(fd, text, strlen(text)) < 0) { /* a test reads what arrived */ }
  close(fd);
}

"#;

/// What the stand-in does once it has started: it holds the pane the way an idle Codex
/// does, and takes the `/quit` of a `feld session stop` as the request to end. Codex's TUI shows
/// the command the first line names and sends it on the next line, so the end takes both
/// lines of the Adapter's `stopInput`.
const CODEX_SOURCE_WAIT: &str = r#"  {
    char line[8192];
    int named = 0;
    while (fgets(line, sizeof line, stdin)) {
      line[strcspn(line, "\r\n")] = 0;
      if (named && line[0] == 0) return 0;
      named = strcmp(line, "/quit") == 0;
    }
  }
  sleep(300);
  return 0;
"#;

/// One string as a C source literal.
fn literal(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// One of the rollout files captured from codex-cli 0.150.0.
fn fixture_rollout(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("codex-0.150.0-{name}.jsonl"))
}

/// Where the stand-in Codex puts the rollout file of one thread, which is where the real
/// one puts it and where the Adapter looks for it.
pub fn codex_rollout_path(config_root: &Path, thread: &str) -> PathBuf {
    config_root
        .join(ROLLOUT_DAY)
        .join(format!("{ROLLOUT_PREFIX}{thread}.jsonl"))
}

/// Waits for the stand-in Codex to record its launch: the argv, then the environment.
pub fn wait_for_codex_launch(record: &Path) -> (Vec<String>, BTreeMap<String, String>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = fs::read_to_string(record).unwrap_or_default();
        if let Some((argv, environment)) = text.split_once("\n\n")
            && text.ends_with('\n')
        {
            return (
                argv.lines().map(str::to_owned).collect(),
                environment
                    .lines()
                    .filter_map(|line| line.split_once('='))
                    .map(|(key, value)| (key.to_owned(), value.to_owned()))
                    .collect(),
            );
        }
        assert!(
            Instant::now() < deadline,
            "codex was not launched: {text:?}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

/// What the stand-in Codex was called with as `codex queue`, one call per argument list.
pub fn codex_queue_calls(calls: &Path) -> Vec<Vec<String>> {
    fs::read_to_string(calls)
        .unwrap_or_default()
        .split("\n\n")
        .filter(|call| !call.trim().is_empty())
        .map(|call| call.lines().map(str::to_owned).collect())
        .collect()
}

/// The variables a herdr pane puts in the processes it holds. A test that runs inside one
/// drops them, so that `self` answers nothing and the fixture's own session is the only
/// herdr anything reaches.
const PANE_VARIABLES: [&str; 4] = [
    "HERDR_ENV",
    "HERDR_SOCKET_PATH",
    "HERDR_PANE_ID",
    "HERDR_SESSION",
];

/// The configuration a fixture server runs with. A pane's shell starts without the login
/// profile of the person running the tests, so whatever that profile puts in front of a
/// new terminal (a multiplexer it attaches to, for one) stays out of the pane, and the
/// line a launch types reaches the shell itself.
const PANE_SHELL_CONFIG: &str = "[terminal]\nshell_mode = \"non_login\"\n";

/// Where the herdr binary comes from: `HERDR_BIN`, or the `herdr` on `PATH`.
const BINARY_VARIABLE: &str = "HERDR_BIN";

/// A headless herdr server of one test alone: its own configuration root, its own session
/// name, and a server this starts, stops, and removes with the root.
///
/// The root sits directly under `/tmp` because herdr puts the socket of a session in it
/// and a Unix socket address holds 104 bytes on macOS, which a temporary directory of the
/// system path spends before the session name is reached.
pub struct HerdrServer {
    binary: PathBuf,
    root: PathBuf,
    session: String,
    server: Option<Child>,
}

impl HerdrServer {
    /// A server of this test's own, or nothing when this machine has no herdr. herdr is
    /// not on every development machine, so the tests that need one step aside with the
    /// reason rather than fail `make check`, unless `FELD_TEST_REQUIRE` names herdr.
    pub fn start(tag: &str) -> Option<Self> {
        let binary = binary()?;
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
        let unique = format!("{}{ordinal}", std::process::id());
        let root = PathBuf::from(format!("/tmp/feldherdr-{tag}-{unique}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("herdr")).expect("create the herdr fixture root");
        fs::write(root.join("herdr/config.toml"), PANE_SHELL_CONFIG)
            .expect("write the herdr fixture config");
        let mut server = Self {
            binary,
            root,
            session: format!("ft{unique}"),
            server: None,
        };
        server.start_server();
        Some(server)
    }

    /// The herdr this fixture runs, as an absolute path.
    pub fn program(&self) -> PathBuf {
        match self.binary.parent() {
            Some(directory) if !directory.as_os_str().is_empty() => self.binary.clone(),
            _ => on_path(&self.binary.to_string_lossy()),
        }
    }

    /// The session name the Placements of this fixture carry.
    pub fn session(&self) -> &str {
        &self.session
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The socket the running server answers on, as herdr itself reports it, which is the
    /// one a pane of it carries in `HERDR_SOCKET_PATH`. A test reads it from herdr rather
    /// than spelling it out, so the fixture follows whatever herdr names it.
    pub fn reported_socket(&self) -> PathBuf {
        let output = self.run(&["status", "server", "--json"]);
        let answer: Value =
            serde_json::from_slice(&output.stdout).expect("decode herdr status JSON");
        PathBuf::from(answer["socket"].as_str().expect("herdr reports a socket"))
    }

    /// Starts the headless server and waits until it answers.
    pub fn start_server(&mut self) {
        let log = fs::File::create(self.root.join("server.log")).expect("open the herdr log");
        let errors = log.try_clone().expect("clone the herdr log handle");
        let mut command = self.command(&["server"]);
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(errors))
            .spawn()
            .expect("start the herdr server");
        self.server = Some(child);
        self.wait_for("running");
    }

    /// Stops the server through its own API, the way a person stops one.
    pub fn stop_server(&mut self) {
        let _ = self.run(&["server", "stop"]);
        self.wait_for("not_running");
        if let Some(mut server) = self.server.take() {
            let _ = server.wait();
        }
    }

    /// One herdr command against this fixture's session, as `Output`.
    pub fn run(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().expect("run herdr")
    }

    /// The `result` of one herdr command, which has to have succeeded.
    pub fn json(&self, arguments: &[&str]) -> Value {
        let output = self.run(arguments);
        let answer = reply(&output).unwrap_or_else(|| {
            panic!(
                "herdr {} answered no JSON:\nstdout: {}\nstderr: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert!(
            answer.get("error").is_none(),
            "herdr {} failed: {answer}",
            arguments.join(" ")
        );
        answer["result"].clone()
    }

    /// The environment a `feld` command needs to reach this server and nothing else: the
    /// herdr binary on `PATH`, this fixture's configuration root, and none of the
    /// variables a pane this test may itself run in would answer `self` with.
    pub fn apply(&self, command: &mut Command) {
        if let Some(directory) = self
            .binary
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            let path = std::env::var("PATH").unwrap_or_default();
            command.env("PATH", format!("{}:{path}", directory.display()));
        }
        for (variable, value) in self.roots() {
            command.env(variable, value);
        }
        for variable in PANE_VARIABLES {
            command.env_remove(variable);
        }
    }

    /// What a shell command inside a pane of this server needs to reach it and nothing
    /// else: the herdr binary on `PATH` and this fixture's roots, as a prefix that a
    /// command run through `sh` can carry.
    pub fn shell_prefix(&self) -> String {
        let mut parts: Vec<String> = self
            .roots()
            .iter()
            .map(|(variable, value)| format!("{variable}='{}'", value.display()))
            .collect();
        if let Some(directory) = self
            .binary
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            parts.push(format!("PATH='{}':\"$PATH\"", directory.display()));
        }
        parts.join(" ")
    }

    /// One herdr command of this fixture, with the server's own environment: the fixture
    /// root is every root herdr reads or writes, so nothing reaches the real
    /// `~/.config/herdr` or the session a person is using.
    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(&self.binary);
        command.env("HOME", &self.root);
        for (variable, value) in self.roots() {
            command.env(variable, value);
        }
        for variable in PANE_VARIABLES {
            command.env_remove(variable);
        }
        // The server is one a person runs, so it holds none of the variables Feld puts in
        // a Session it owns, and neither do the panes it opens.
        without_outside_variables(&mut command);
        command.arg("--session").arg(&self.session).args(arguments);
        command
    }

    fn roots(&self) -> [(&'static str, PathBuf); 4] {
        [
            ("XDG_CONFIG_HOME", self.root.clone()),
            ("XDG_STATE_HOME", self.root.join("state")),
            ("XDG_DATA_HOME", self.root.join("data")),
            ("XDG_CACHE_HOME", self.root.join("cache")),
        ]
    }

    fn wait_for(&self, status: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let output = self.run(&["status", "server", "--json"]);
            let answered = serde_json::from_slice::<Value>(&output.stdout)
                .ok()
                .and_then(|answer| answer["status"].as_str().map(str::to_owned));
            if answered.as_deref() == Some(status) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the herdr server of session {} did not become {status} (it is {answered:?}); log:\n{}",
                self.session,
                fs::read_to_string(self.root.join("server.log")).unwrap_or_default()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for HerdrServer {
    fn drop(&mut self) {
        let _ = self.run(&["server", "stop"]);
        if let Some(mut server) = self.server.take() {
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The herdr binary the tests drive, or nothing with the reason on stderr. A machine
/// without one fails the test when `FELD_TEST_REQUIRE` names herdr.
fn binary() -> Option<PathBuf> {
    // Read the variable before anything else, so that a misspelt name fails here too.
    test_require::required("herdr");
    let named = std::env::var_os(BINARY_VARIABLE).map(PathBuf::from);
    let binary = named.clone().unwrap_or_else(|| PathBuf::from("herdr"));
    if Command::new(&binary)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return Some(binary);
    }
    test_require::step_aside(
        "herdr",
        &format!(
            "no herdr to run ({}). Install herdr, or name one with {BINARY_VARIABLE}",
            match named {
                Some(path) => format!("{BINARY_VARIABLE}={}", path.display()),
                None => "none on PATH".to_owned(),
            }
        ),
    );
    None
}

/// The one JSON object a herdr command answers, which it prints on stdout when it
/// succeeded and on stderr when it failed.
fn reply(output: &Output) -> Option<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    [stdout, stderr]
        .iter()
        .flat_map(|printed| printed.lines())
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .find(|answer| answer.get("id").is_some())
}

/// A valid OTLP metrics export whose encoding is exactly `length` bytes, which the schema URL
/// of a resource pads out and empty resources fill in where the padding's length prefix grows.
pub fn metrics_export_of_length(length: usize) -> Vec<u8> {
    (0..3)
        .find_map(|empty| {
            (0..=length).rev().take(16).find_map(|padding| {
                let padded = proto::ResourceMetrics {
                    resource: None,
                    scope_metrics: Vec::new(),
                    schema_url: "x".repeat(padding),
                };
                let request = proto::ExportMetricsServiceRequest {
                    resource_metrics: std::iter::once(padded)
                        .chain(std::iter::repeat_n(
                            proto::ResourceMetrics::default(),
                            empty,
                        ))
                        .collect(),
                };
                (request.encoded_len() == length).then(|| request.encode_to_vec())
            })
        })
        .expect("requested protobuf size cannot be represented")
}

/// Stand-ins for the products a Provider drives, which write every argv they are run with
/// into a log and run the real program with it. A Provider hands a pane's shell its script
/// through these command lines too, so the log holds every command line of a launch that
/// `ps` shows every user of the machine.
pub struct ArgvLog {
    directory: tempfile::TempDir,
}

impl ArgvLog {
    /// Stand-ins for `programs`, each named after the real program it runs, warmed up the
    /// way [`warm_up_plugins`] warms up a Plugin.
    pub fn wrapping(programs: &[&Path]) -> Self {
        let directory = tempfile::tempdir().expect("create the argv log directory");
        let log = directory.path().join("argv.log");
        for program in programs {
            let wrapper = directory
                .path()
                .join(program.file_name().expect("a program name"));
            fs::write(
                &wrapper,
                format!(
                    "#!/bin/sh\n[ -n \"$FELD_TEST_WARM_UP\" ] && exit 0\nprintf '%s\\n' \"$*\" >> {}\nexec {} \"$@\"\n",
                    feld_core::process::quote(&log.to_string_lossy()),
                    feld_core::process::quote(&program.to_string_lossy()),
                ),
            )
            .expect("write a stand-in");
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))
                .expect("make a stand-in runnable");
            warm_up(Command::new(&wrapper).env("FELD_TEST_WARM_UP", "1"));
        }
        Self { directory }
    }

    /// `command`, with the stand-ins ahead of every other program on its PATH.
    pub fn apply(&self, command: &mut Command) {
        let path = command
            .get_envs()
            .find(|(name, _)| *name == "PATH")
            .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
            .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
        command.env(
            "PATH",
            format!("{}:{path}", self.directory.path().display()),
        );
    }

    /// The command lines the stand-ins were run with that hold `value`.
    pub fn holding(&self, value: &str) -> Vec<String> {
        fs::read_to_string(self.directory.path().join("argv.log"))
            .expect("read the argv log")
            .lines()
            .filter(|line| line.contains(value))
            .map(str::to_owned)
            .collect()
    }
}

/// The program `name` resolves to on PATH.
pub fn on_path(name: &str) -> PathBuf {
    let found = Command::new("which").arg(name).output().expect("run which");
    assert!(found.status.success(), "{name} is not on PATH");
    PathBuf::from(String::from_utf8_lossy(&found.stdout).trim())
}

/// What a program wrote into `path`, once it has written something.
pub fn wait_for_written(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(written) = fs::read_to_string(path)
            && !written.is_empty()
        {
            return written;
        }
        assert!(
            Instant::now() < deadline,
            "nothing was written into {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// The launch handed the environment over without writing it into the temporary directory,
/// which a sandboxed Runtime of another Session reads and writes.
pub fn assert_no_env_file_holds(value: &str) {
    for entry in fs::read_dir(std::env::temp_dir()).expect("read the temporary directory") {
        let path = entry.expect("read a temporary entry").path();
        let is_launch_env = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("feld-env-"));
        if !is_launch_env {
            continue;
        }
        let held = match fs::read_dir(&path) {
            Ok(entries) => entries.flatten().any(|entry| {
                fs::read_to_string(entry.path()).is_ok_and(|held| held.contains(value))
            }),
            Err(_) => fs::read_to_string(&path).is_ok_and(|held| held.contains(value)),
        };
        assert!(!held, "{} holds the launch environment", path.display());
    }
}
