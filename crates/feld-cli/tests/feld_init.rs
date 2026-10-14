//! What `feld init` leaves behind: a Feld home whose `plugins/` holds the Plugins Feld
//! ships, chosen kind by kind, and which a daemon then lists as installed.

use std::fs;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

use support::stand_in::write_stand_in;

const FELD: &str = env!("CARGO_BIN_EXE_feld");

struct Fixture {
    temp: TempDir,
}

impl Fixture {
    /// A Feld home nothing has written yet, so that `feld init` makes it, with Claude
    /// Code and tmux on `PATH`.
    fn new() -> Self {
        Self::on(tempfile::tempdir().expect("create temporary Feld home"))
    }

    /// A Feld home with a short path, so that the lines naming it fit a narrow pane.
    fn short() -> Self {
        Self::on(
            tempfile::Builder::new()
                .prefix("fi")
                .tempdir_in("/tmp")
                .expect("create temporary Feld home"),
        )
    }

    fn on(temp: TempDir) -> Self {
        let fixture = Self { temp };
        fixture.with_on_path(&["claude", "tmux"]);
        fixture.with_spool_writable(&fixture.home().join(".codex"));
        fixture
    }

    /// Lets the sandbox of the Codex of configuration root `root` write every Feld home of
    /// the fixture, so that preparing it adds nothing that asks for consent.
    fn with_spool_writable(&self, root: &Path) {
        fs::create_dir_all(root).expect("create a Codex configuration root");
        fs::write(
            root.join("config.toml"),
            format!(
                "[sandbox_workspace_write]\nwritable_roots = [\"{}\"]\n",
                fs::canonicalize(self.home())
                    .expect("resolve the fixture HOME")
                    .display()
            ),
        )
        .expect("write a Codex config.toml");
    }

    fn home(&self) -> &Path {
        self.temp.path()
    }

    /// The only directory on the `PATH` of `feld`, so that what `feld init` finds there
    /// is what the test puts there rather than what the machine has.
    fn path(&self) -> std::path::PathBuf {
        self.home().join("path")
    }

    /// Puts a stand-in for each of `commands`, and nothing else, on the `PATH` of `feld`.
    fn with_on_path(&self, commands: &[&str]) {
        let path = self.path();
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create the PATH directory");
        for command in commands {
            write_stand_in(&path.join(command), "exit 1");
        }
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.feld_with(arguments, &[])
    }

    /// Runs `feld` as `feld` does, with `variables` set on top.
    fn feld_with(&self, arguments: &[&str], variables: &[(&str, &str)]) -> Output {
        self.feld_at(self.home(), arguments, variables)
    }

    /// Runs `feld` on the Feld home `feld_home`, with the same `HOME` as the others.
    fn feld_at(&self, feld_home: &Path, arguments: &[&str], variables: &[(&str, &str)]) -> Output {
        Command::new(FELD)
            .arg("--home")
            .arg(feld_home)
            // Nothing here may reach the real `~/.feld`, `~/.claude` or `~/.zshrc`.
            .env("HOME", self.home())
            .env("PATH", self.path())
            .env("SHELL", "/bin/zsh")
            .env_remove("ZDOTDIR")
            .env_remove("FELD_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            // Neither the Session nor the pane this test runs inside speaks for the fixture.
            .env_remove("FELD_API")
            .env_remove("FELD_SESSION_TOKEN")
            .env_remove("FELD_WORKSPACE")
            .env_remove("FELD_ACTOR")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("HERDR_ENV")
            .env_remove("HERDR_PANE_ID")
            .env_remove("HERDR_SESSION")
            .env_remove("HERDR_SOCKET_PATH")
            .envs(variables.iter().copied())
            .args(arguments)
            .output()
            .expect("run feld")
    }

    fn succeeds(&self, arguments: &[&str]) -> String {
        let output = self.feld(arguments);
        assert!(
            output.status.success(),
            "feld {} failed:\nstdout: {}\nstderr: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn json(&self, arguments: &[&str]) -> Value {
        serde_json::from_str(&self.succeeds(arguments)).expect("parse feld JSON output")
    }

    fn plugin_dir(&self, kind: &str, name: &str) -> std::path::PathBuf {
        self.home().join("plugins").join(kind).join(name)
    }

    /// Runs `feld` with a terminal on its standard streams, so that it draws the list and
    /// takes keys, and answers with the terminal and the exit code.
    fn on_a_terminal(&self, arguments: &[&str]) -> Terminal {
        self.on_a_terminal_with(arguments, &[])
    }

    /// [`Fixture::on_a_terminal`] with more variables in the environment.
    fn on_a_terminal_with(&self, arguments: &[&str], variables: &[(&str, &str)]) -> Terminal {
        let (controller, device) = pty();
        let child = Command::new(FELD)
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .env("PATH", self.path())
            .env("SHELL", "/bin/zsh")
            .env_remove("ZDOTDIR")
            .env("TERM", "xterm-256color")
            .env("LANG", "en_US.UTF-8")
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR_FORCE")
            .env_remove("FELD_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            // Neither the Session nor the pane this test runs inside speaks for the fixture.
            .env_remove("FELD_API")
            .env_remove("FELD_SESSION_TOKEN")
            .env_remove("FELD_WORKSPACE")
            .env_remove("FELD_ACTOR")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .envs(variables.iter().copied())
            .args(arguments)
            .stdin(Stdio::from(device.try_clone().expect("clone the terminal")))
            .stdout(Stdio::from(device.try_clone().expect("clone the terminal")))
            .stderr(Stdio::from(device))
            .spawn()
            .expect("run feld on a terminal");
        Terminal::new(controller, child)
    }
}

/// A terminal pair: what `feld` is given, and what the test reads and types on.
fn pty() -> (OwnedFd, OwnedFd) {
    let mut controller = 0;
    let mut device = 0;
    let made = unsafe {
        libc::openpty(
            &mut controller,
            &mut device,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(
        made,
        0,
        "open a terminal: {}",
        std::io::Error::last_os_error()
    );
    unsafe {
        (
            OwnedFd::from_raw_fd(controller),
            OwnedFd::from_raw_fd(device),
        )
    }
}

/// The side of the terminal the test sits on: everything `feld` has drawn so far, and the
/// keys the test presses.
struct Terminal {
    keys: std::fs::File,
    seen: Arc<Mutex<String>>,
    reader: thread::JoinHandle<()>,
    child: std::process::Child,
}

impl Terminal {
    fn new(controller: OwnedFd, child: std::process::Child) -> Self {
        let keys = std::fs::File::from(controller.try_clone().expect("clone the terminal"));
        let seen = Arc::new(Mutex::new(String::new()));
        let drawn = Arc::clone(&seen);
        let reader = thread::spawn(move || {
            let mut screen = std::fs::File::from(controller);
            let mut buffer = [0u8; 4096];
            // A terminal whose other side is gone reads as an error on Linux and as the
            // end of the stream on macOS.
            while let Ok(read) = screen.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                drawn
                    .lock()
                    .expect("the drawn screen")
                    .push_str(&String::from_utf8_lossy(&buffer[..read]));
            }
        });
        Self {
            keys,
            seen,
            reader,
            child,
        }
    }

    /// Waits for `feld` to have drawn `wanted`, read without its colors, so that a key
    /// lands on the ask it is in. An ask after `feld` has installed or run the Plugins
    /// comes seconds later under load, while macOS checks the executables `feld init` put.
    fn waits_for(&self, wanted: &str) {
        let until = Instant::now() + Duration::from_secs(60);
        while Instant::now() < until {
            if self.text().contains(wanted) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("the ask never drew {wanted:?}:\n{}", self.text());
    }

    /// Answers no to adding the `feld env` line, which `feld init` asks after it has
    /// installed and prepared.
    fn declines_the_shell_line(&mut self) {
        self.waits_for("Add this line to your shell settings?");
        self.presses(b"n");
    }

    fn presses(&mut self, key: &[u8]) {
        self.keys.write_all(key).expect("press a key");
        self.keys.flush().expect("press a key");
    }

    fn drawn(&self) -> String {
        self.seen.lock().expect("the drawn screen").clone()
    }

    /// What `feld` has drawn so far, without the escape sequences that color it and move
    /// the cursor.
    fn text(&self) -> String {
        uncolored(&self.drawn())
    }

    /// Waits for `feld` to end and for what it drew last to be read, and answers with its
    /// exit code.
    fn ends(&mut self) -> Option<i32> {
        let code = self.child.wait().expect("wait for feld").code();
        let until = Instant::now() + Duration::from_secs(5);
        while !self.reader.is_finished() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        code
    }
}

impl Drop for Terminal {
    /// A test that ends while the list is still up leaves no `feld` behind.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
    }
}

/// Text without its escape sequences.
fn uncolored(drawn: &str) -> String {
    let mut text = String::new();
    let mut chars = drawn.chars();
    while let Some(char) = chars.next() {
        if char != '\x1b' {
            text.push(char);
        } else if chars.next() == Some('[') {
            chars.by_ref().find(|byte| ('\x40'..='\x7e').contains(byte));
        }
    }
    text
}

fn names(plugins: &Value) -> Vec<String> {
    plugins
        .as_array()
        .expect("a Plugin list")
        .iter()
        .map(|plugin| plugin["name"].as_str().expect("a Plugin name").to_owned())
        .collect()
}

/// Whether a Plugin directory holds everything core and Claude Code read in it.
fn is_complete(directory: &Path) -> bool {
    directory.join("plugin.json").is_file() && is_runnable(&directory.join("plugin"))
}

#[cfg(unix)]
fn is_runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|data| data.is_file() && data.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_runnable(path: &Path) -> bool {
    path.is_file()
}

#[test]
fn init_installs_what_feld_ships_and_the_daemon_lists_it() {
    let fixture = Fixture::new();
    let zshrc = fixture.home().join(".zshrc");

    // What is not a terminal reads each step as one plain `key: value` line, and `--yes`
    // starts the daemon as well.
    let printed = fixture.succeeds(&["init", "--yes"]);
    assert!(
        printed.starts_with(&format!(
            "Feld v{}\nFeld home:   {}\nPlugins:     runtimes: claude-code  providers: tmux\nClaude Code: prepared\nShell:       added eval \"$(feld env)\" to {}\nDaemon:      started \u{b7} pid ",
            env!("CARGO_PKG_VERSION"),
            fixture.home().display(),
            zshrc.display()
        )),
        "{printed}"
    );
    assert!(!printed.contains('\x1b'), "{printed}");
    // The end runs the commands to type one to a line, the Web UI, the Runtime's notes, and
    // where to turn when something does not work.
    let web_ui = fixture.json(&["--json", "daemon", "status"])["webUi"]
        .as_str()
        .expect("the Web UI")
        .to_owned();
    assert!(
        printed.ends_with(&format!(
            "Feld is ready.\n  next:  source {}\n  next:  feld ui\n  ui:    {web_ui}  \u{b7}  first time in this browser: run `feld ui` to sign in\n  note:  Claude Code: run /reload-plugins in a Claude Code that is already open\n  note:  Claude Code: restart a Claude Code started before this init to send its metrics\n  note:  run `feld doctor` if something does not work\n",
            zshrc.display()
        )),
        "{printed}"
    );

    // The Plugin directories hold the manifest, the executable core runs, and the files the
    // Runtime's own parts are reached through.
    let claude_code = fixture.plugin_dir("runtime", "claude-code");
    let tmux = fixture.plugin_dir("provider", "tmux");
    assert!(is_complete(&claude_code));
    assert!(is_complete(&tmux));
    for outside in [("runtime", "codex"), ("provider", "herdr")] {
        assert!(
            !fixture.plugin_dir(outside.0, outside.1).exists(),
            "a shipped Plugin outside the default is left for whoever names it"
        );
    }
    for shim in ["hook", "monitor", "otel-headers", "status-line"] {
        assert!(
            is_runnable(&claude_code.join("bin").join(shim)),
            "the {shim} shim is runnable"
        );
    }
    for declaration in [
        ".claude-plugin/plugin.json",
        "hooks/hooks.json",
        "monitors/monitors.json",
        "skills/feld/SKILL.md",
    ] {
        assert!(claude_code.join(declaration).is_file(), "{declaration}");
    }

    // The daemon init started reads them as installed Plugins.
    let started = fixture.json(&["--json", "daemon", "start"]);
    assert_eq!(started["alreadyRunning"], true);
    assert_eq!(started["daemon"]["state"], "running");
    // `feld daemon start` shows the Web UI as well, and how a browser signs in rather than
    // a pairing code.
    assert_eq!(started["ui"], web_ui.as_str());
    let shown = fixture.succeeds(&["daemon", "start"]);
    assert!(
        shown.contains(&format!(
            "\n  ui:         {web_ui}  \u{b7}  first time in this browser: run `feld ui` to sign in\n"
        )),
        "{shown}"
    );
    assert!(!shown.contains("code"), "{shown}");
    let status = fixture.json(&["--json", "daemon", "status"]);
    assert_eq!(names(&status["runtimes"]), ["claude-code"]);
    assert_eq!(names(&status["providers"]), ["tmux"]);
    let human = fixture.succeeds(&["daemon", "status"]);
    assert!(human.contains("runtimes:   claude-code"), "{human}");
    assert!(human.contains("providers:  tmux"), "{human}");

    // With a daemon running, the Plugin directories take effect on a restart, which
    // `--yes` makes; `--json` leaves the daemon alone and names the restart.
    let again = fixture.json(&["--json", "init", "--yes"]);
    assert_eq!(again["next"], "feld daemon restart");
    assert_eq!(again["ui"], web_ui.as_str());
    let restarted = fixture.succeeds(&["init", "--yes"]);
    assert!(
        restarted.contains("Daemon:      restarted \u{b7} pid "),
        "{restarted}"
    );
}

/// `--yes` adds the `feld env` line once, under a comment, to the end of the file an
/// interactive zsh or bash reads, and a re-run finds it there.
#[test]
fn yes_adds_the_feld_env_line_to_the_shell_settings_once() {
    let fixture = Fixture::new();
    let zshrc = fixture.home().join(".zshrc");
    fs::write(&zshrc, "alias ll='ls -l'").unwrap();

    let added = fixture.json(&["--json", "init", "--yes"]);
    assert_eq!(added["shell"], json!({"path": zshrc, "status": "added"}));
    assert_eq!(
        fs::read_to_string(&zshrc).unwrap(),
        "alias ll='ls -l'\n\n# Added by `feld init`: connects this shell to the Feld daemon\neval \"$(feld env)\"\n"
    );
    let again = fixture.succeeds(&["init", "--yes"]);
    assert!(
        again.contains(&format!(
            "Shell:       eval \"$(feld env)\" already in {}\n",
            zshrc.display()
        )),
        "{again}"
    );
    assert_eq!(feld_env_lines(&zshrc), 1);

    // zsh reads `.zshrc` under `ZDOTDIR`, and bash `~/.bashrc`.
    let dot = Fixture::new();
    let zdotdir = dot.home().join("zsh");
    let output = dot.feld_with(
        &["--json", "init", "--yes"],
        &[("ZDOTDIR", &zdotdir.display().to_string())],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(feld_env_lines(&zdotdir.join(".zshrc")), 1);
    assert!(!dot.home().join(".zshrc").exists());
    let bash = Fixture::new();
    let output = bash.feld_with(
        &["--json", "init", "--yes"],
        &[("SHELL", "/usr/local/bin/bash")],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(feld_env_lines(&bash.home().join(".bashrc")), 1);
    assert!(!bash.home().join(".zshrc").exists());

    // A line in another file the shell reads is found there; a file of another shell is not.
    let profile = Fixture::new();
    let zprofile = profile.home().join(".zprofile");
    fs::write(&zprofile, "eval \"$(feld env)\"\n").unwrap();
    let found = profile.json(&["--json", "init", "--yes"]);
    assert_eq!(
        found["shell"],
        json!({"path": zprofile, "status": "present"})
    );
    assert!(!profile.home().join(".zshrc").exists());
    let output = profile.feld_with(&["--json", "init", "--yes"], &[("SHELL", "/bin/bash")]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(feld_env_lines(&profile.home().join(".bashrc")), 1);

    // A comment, an alias, or a command that only names `feld env` reads nothing.
    let named = Fixture::new();
    let zshrc = named.home().join(".zshrc");
    fs::write(
        &zshrc,
        "# unrelated note: run feld env after login\necho feld env\n  # eval \"$(feld env)\"\nalias feldconnect='eval \"$(feld env)\"'\n",
    )
    .unwrap();
    let added = named.json(&["--json", "init", "--yes"]);
    assert_eq!(added["shell"], json!({"path": zshrc, "status": "added"}));
    assert!(
        fs::read_to_string(&zshrc)
            .unwrap()
            .ends_with("\neval \"$(feld env)\"\n")
    );

    // With `ZDOTDIR`, zsh reads its files there and not the ones under HOME.
    let moved = Fixture::new();
    fs::write(moved.home().join(".zshrc"), "eval \"$(feld env)\"\n").unwrap();
    let zdotdir = moved.home().join("zsh");
    let output = moved.feld_with(
        &["--json", "init", "--yes"],
        &[("ZDOTDIR", &zdotdir.display().to_string())],
    );
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        result["shell"],
        json!({"path": zdotdir.join(".zshrc"), "status": "added"})
    );
    assert_eq!(feld_env_lines(&zdotdir.join(".zshrc")), 1);
}

/// The `source` the end shows runs as it is, quoted when the path needs it.
#[test]
fn the_source_command_quotes_a_path_with_spaces() {
    let fixture = Fixture::on(
        tempfile::Builder::new()
            .prefix("feld init ")
            .tempdir()
            .expect("create temporary Feld home"),
    );
    let printed = fixture.succeeds(&["init", "--yes"]);
    assert!(
        printed.contains(&format!(
            "\n  next:  source '{}'\n",
            fixture.home().join(".zshrc").display()
        )),
        "{printed}"
    );
}

/// Without consent or with a shell other than zsh and bash, the shell settings stay as they
/// are, Feld is ready all the same, and a note says what line to add.
#[test]
fn the_feld_env_line_waits_for_consent_and_a_known_shell() {
    let fixture = Fixture::new();
    let zshrc = fixture.home().join(".zshrc");
    let note = format!(
        "note:  add `eval \"$(feld env)\"` to {} to connect your shell to Feld",
        zshrc.display()
    );

    // What is not a terminal adds it only with `--yes`.
    let printed = fixture.succeeds(&["init"]);
    assert!(
        printed.contains("Shell:       not added (needs your consent)\n"),
        "{printed}"
    );
    let (_, end) = printed.split_once("Feld is ready.\n").expect("ready");
    assert!(end.contains(&format!("\n  {note}\n")), "{printed}");
    assert!(!end.contains("source"), "{printed}");
    assert!(!zshrc.exists());

    // A terminal that says no.
    let mut terminal = fixture.on_a_terminal(&["init"]);
    terminal.waits_for("Choose the Plugins to install");
    terminal.presses(b"\r");
    terminal.declines_the_shell_line();
    terminal.waits_for("Start the daemon now?");
    terminal.presses(b"n");
    assert_eq!(terminal.ends(), Some(0));
    let text = terminal.text();
    assert!(
        text.contains("\u{25c7} Shell  not added (declined)"),
        "{text}"
    );
    assert!(text.contains(&format!("\n   {note}\r\n")), "{text}");
    assert!(!zshrc.exists());

    // fish is not asked about, even with `--yes`.
    let fish = [("SHELL", "/opt/homebrew/bin/fish")];
    let output = fixture.feld_with(&["init"], &fish);
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        printed.contains("Shell:       not added (fish is not zsh or bash)\n"),
        "{printed}"
    );
    let result = serde_json::from_slice::<Value>(
        &fixture
            .feld_with(&["--json", "init", "--yes"], &fish)
            .stdout,
    )
    .expect("parse feld JSON output");
    assert_eq!(
        result["shell"],
        json!({"path": null, "status": "unsupported"})
    );
    assert_eq!(
        result["notes"],
        json!([
            "add `eval \"$(feld env)\"` to the settings file of your shell to connect it to Feld"
        ])
    );
    assert_eq!(result["ui"], Value::Null);
    for file in [
        ".zshrc",
        ".bashrc",
        ".zprofile",
        ".bash_profile",
        ".profile",
    ] {
        assert!(!fixture.home().join(file).exists(), "{file}");
    }
}

#[test]
fn init_says_a_tmux_older_than_3_2_is_not_supported() {
    let fixture = Fixture::new();
    let bin = fixture.home().join("bin");
    fs::create_dir(&bin).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let init = |version: &str, arguments: &[&str]| {
        write_stand_in(&bin.join("tmux"), &format!("echo 'tmux {version}'"));
        let output = fixture.feld_with(arguments, &[("PATH", &path)]);
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let plain = ["init", "--runtime", "claude-code", "--provider", "tmux"];
    let json = [
        "--json",
        "init",
        "--runtime",
        "claude-code",
        "--provider",
        "tmux",
    ];

    let old = init("3.1c", &plain);
    assert!(
        old.contains(
            "warning: tmux 3.1c is not supported; the tmux Provider needs tmux 3.2 or later"
        ),
        "{old}"
    );
    assert!(old.contains("Feld is ready."), "{old}");
    let old: Value = serde_json::from_str(&init("3.1c", &json)).unwrap();
    assert_eq!(
        old["warnings"],
        json!(["tmux 3.1c is not supported; the tmux Provider needs tmux 3.2 or later"])
    );

    // A supported tmux, and one whose version does not read as a number, pass.
    for version in ["3.2a", "3.6a", "next-3.7", "openbsd-7.5"] {
        assert!(!init(version, &plain).contains("warning:"), "{version}");
        let answer: Value = serde_json::from_str(&init(version, &json)).unwrap();
        assert_eq!(answer["warnings"], json!([]), "{version}");
    }
}

/// Each product older than its oldest supported version is told on a line of its own, and
/// init goes on.
#[test]
fn init_says_each_product_older_than_its_oldest_supported_version_is_not_supported() {
    let fixture = Fixture::new();
    let bin = fixture.home().join("bin");
    fs::create_dir(&bin).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let init = |versions: [&str; 4], arguments: &[&str]| {
        for (command, answer) in ["claude", "codex", "tmux", "herdr"].iter().zip([
            format!("{} (Claude Code)", versions[0]),
            format!("codex-cli {}", versions[1]),
            format!("tmux {}", versions[2]),
            format!("herdr {}", versions[3]),
        ]) {
            write_stand_in(&bin.join(command), &format!("echo '{answer}'"));
        }
        let output = fixture.feld_with(arguments, &[("PATH", &path)]);
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let every = [
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
        "--provider",
        "herdr",
    ];
    let json: Vec<&str> = ["--json"].into_iter().chain(every).collect();
    let told = [
        "Claude Code 2.1.150 is not supported; the Claude Code Runtime Adapter needs Claude Code 2.1.181 or later",
        "Codex 0.155.1 is not supported; the Codex Runtime Adapter needs Codex 0.156.0 or later",
        "herdr 0.8.2 is not supported; the herdr Provider needs herdr 0.9.0 or later",
    ];

    let old = ["2.1.150", "0.155.1", "3.6a", "0.8.2"];
    let printed = init(old, &every);
    for warning in told {
        assert!(
            printed.contains(&format!("warning: {warning}")),
            "{printed}"
        );
    }
    assert!(printed.contains("Feld is ready."), "{printed}");
    let answer: Value = serde_json::from_str(&init(old, &json)).unwrap();
    let mut warnings: Vec<&str> = answer["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|warning| warning.as_str().unwrap())
        .collect();
    warnings.sort_unstable();
    assert_eq!(warnings, told);

    let oldest = ["2.1.181", "0.156.0", "3.2a", "0.9.0"];
    assert!(!init(oldest, &every).contains("warning:"));
    let answer: Value = serde_json::from_str(&init(oldest, &json)).unwrap();
    assert_eq!(answer["warnings"], json!([]));
}

/// A shipped Plugin outside the default is there for whoever names it, and the daemon lists
/// it beside the default one.
#[test]
fn a_plugin_outside_the_default_is_installed_when_the_answer_names_it() {
    let fixture = Fixture::new();

    let chosen = fixture.json(&[
        "--json",
        "init",
        "--runtime",
        "claude-code",
        "--provider",
        "tmux",
        "--provider",
        "herdr",
    ]);
    assert_eq!(
        chosen["chosen"],
        serde_json::json!(["claude-code", "tmux", "herdr"])
    );
    assert!(is_complete(&fixture.plugin_dir("provider", "herdr")));
    assert_eq!(names(&chosen["providers"]), ["herdr", "tmux"]);

    let started = fixture.json(&["--json", "daemon", "start"]);
    assert_eq!(started["daemon"]["state"], "running");
    let status = fixture.json(&["--json", "daemon", "status"]);
    assert_eq!(names(&status["providers"]), ["herdr", "tmux"]);
    let human = fixture.succeeds(&["daemon", "status"]);
    assert!(human.contains("providers:  herdr, tmux"), "{human}");

    // With two Providers installed and nothing to tell them apart, a creation is refused.
    let refused = fixture.feld(&["actor", "create", "one", "--kind", "job", "--", "true"]);
    assert!(!refused.status.success());
    let reason = String::from_utf8_lossy(&refused.stderr);
    assert!(
        reason.contains("which Execution Provider"),
        "{reason}{}",
        String::from_utf8_lossy(&refused.stdout)
    );

    // Naming one of them, or the option prefix of one, resolves it again.
    let named = fixture.json(&[
        "--json",
        "actor",
        "create",
        "two",
        "--kind",
        "job",
        "--provider",
        "tmux",
        "--",
        "true",
    ]);
    assert_eq!(named["actor"]["launchSpec"]["provider"], "tmux");
    let by_prefix = fixture.json(&[
        "--json",
        "actor",
        "create",
        "three",
        "--kind",
        "job",
        "--herdr-unit",
        "workspace",
        "--",
        "true",
    ]);
    assert_eq!(by_prefix["actor"]["launchSpec"]["provider"], "herdr");
}

#[test]
fn each_kind_takes_its_own_answer_and_keeps_what_the_answer_leaves_out() {
    let fixture = Fixture::new();

    // A flag is the answer for its kind, and the kind it says nothing about stays empty.
    let runtime_only = fixture.json(&["--json", "init", "--runtime", "claude-code"]);
    assert_eq!(runtime_only["chosen"], serde_json::json!(["claude-code"]));
    assert_eq!(names(&runtime_only["runtimes"]), ["claude-code"]);
    assert!(names(&runtime_only["providers"]).is_empty());
    assert!(!fixture.plugin_dir("provider", "tmux").exists());
    assert_eq!(runtime_only["next"], "feld init --provider tmux");

    // The next answer installs the other kind, and leaves the installed one alone.
    let stale = fixture.plugin_dir("runtime", "claude-code").join("stale");
    fs::write(&stale, "from an older Feld").unwrap();
    let provider_only = fixture.json(&["--json", "init", "--provider", "tmux"]);
    assert_eq!(provider_only["chosen"], serde_json::json!(["tmux"]));
    assert_eq!(names(&provider_only["runtimes"]), ["claude-code"]);
    assert_eq!(names(&provider_only["providers"]), ["tmux"]);
    assert!(
        stale.is_file(),
        "a Plugin the answer left out is kept as it is"
    );

    // Installing a Plugin again writes its directory afresh.
    fixture.json(&["--json", "init", "--runtime", "claude-code"]);
    assert!(!stale.exists(), "a chosen Plugin is put down anew");
    assert!(is_complete(&fixture.plugin_dir("runtime", "claude-code")));
    assert!(is_complete(&fixture.plugin_dir("provider", "tmux")));

    // Only what Feld ships can be chosen.
    let unknown = fixture.feld(&["init", "--runtime", "aider"]);
    assert_eq!(unknown.status.code(), Some(1));
    let refusal = String::from_utf8_lossy(&unknown.stderr);
    assert!(
        refusal.contains("Feld ships no runtime Plugin named 'aider'"),
        "{refusal}"
    );
}

/// A kind with nothing installed reads as `(none)`, and each step names the `feld init`
/// that places the kind's default.
#[test]
fn a_kind_with_nothing_installed_names_the_init_that_places_it() {
    let fixture = Fixture::new();
    let printed = fixture.succeeds(&["init", "--runtime", "codex"]);
    assert!(
        printed.ends_with(
            "Plugins:     runtimes: codex  providers: (none)\nCodex:       prepared\nFeld is not ready.\n  next:  feld init --provider tmux\n"
        ),
        "{printed}"
    );
    let started = fixture.succeeds(&["daemon", "start"]);
    assert!(started.contains("  providers:  (none)"), "{started}");
    assert!(!started.contains("  runtimes:"), "{started}");
    let status = fixture.succeeds(&["daemon", "status"]);
    assert!(
        status.contains("  runtimes:   codex\n  providers:  (none)"),
        "{status}"
    );

    // The CLI refuses a creation or an attach it cannot place, and the daemon refuses a named
    // Provider.
    let guidance = "No Execution Providers are installed; run `feld init --provider tmux`";
    for arguments in [
        &["actor", "create", "one", "--kind", "job", "--", "true"][..],
        &[
            "actor",
            "create",
            "one",
            "--kind",
            "job",
            "--provider",
            "tmux",
            "--",
            "true",
        ][..],
        &[
            "actor",
            "attach",
            "a1",
            "--runtime",
            "codex",
            "--tmux-pane",
            "%0",
        ][..],
    ] {
        let refused = fixture.feld(arguments);
        assert_eq!(refused.status.code(), Some(1));
        let reason = String::from_utf8_lossy(&refused.stderr);
        assert!(reason.contains(guidance), "{reason}");
    }

    let providers_only = Fixture::new();
    let printed = providers_only.json(&["--json", "init", "--provider", "tmux"]);
    assert_eq!(printed["next"], "feld init --runtime claude-code");
    providers_only.succeeds(&["daemon", "start"]);
    let refused = providers_only.feld(&[
        "actor",
        "create",
        "one",
        "--kind",
        "agent",
        "--runtime",
        "codex",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    let reason = String::from_utf8_lossy(&refused.stderr);
    assert!(
        reason.contains("No Runtime Adapters are installed; run `feld init --runtime claude-code`"),
        "{reason}"
    );
    let refused = providers_only.feld(&[
        "actor",
        "attach",
        "a1",
        "--runtime",
        "codex",
        "--tmux-pane",
        "%0",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    let reason = String::from_utf8_lossy(&refused.stderr);
    assert!(
        reason.contains("No Runtime Adapters are installed; run `feld init --runtime claude-code`"),
        "{reason}"
    );
    let status = providers_only.json(&["--json", "daemon", "status"]);
    let token = fs::read_to_string(providers_only.home().join("admin/api-token")).unwrap();
    let refused = reqwest::blocking::Client::new()
        .post(format!(
            "{}/v1/workspaces/default/actors/a1/attach",
            status["api"].as_str().unwrap()
        ))
        .bearer_auth(token.trim())
        .json(&json!({
            "runtime": "codex",
            "placement": {"provider": "tmux", "node": "local", "workdir": "/", "placement": {}}
        }))
        .send()
        .unwrap();
    assert_eq!(refused.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(
        refused.json::<Value>().unwrap()["message"],
        "No Runtime Adapters are installed; run `feld init --runtime claude-code`"
    );
}

/// A re-run lists the Plugins installed once it is done, where one the run left out is
/// marked as kept.
#[test]
fn a_re_run_marks_what_it_left_installed_as_kept() {
    let fixture = Fixture::new();
    fixture.succeeds(&["init", "--yes"]);

    let printed = fixture.succeeds(&["init", "--runtime", "codex", "--provider", "tmux"]);
    assert!(
        printed.contains("Plugins:     runtimes: claude-code (kept), codex  providers: tmux\n"),
        "{printed}"
    );
    // The daemon `--yes` started takes the Plugins on a restart, which nobody asked for.
    assert!(
        printed.contains("  next:  feld daemon restart"),
        "{printed}"
    );

    let json = fixture.json(&["--json", "init", "--runtime", "codex", "--provider", "tmux"]);
    assert_eq!(json["chosen"], serde_json::json!(["codex", "tmux"]));
    assert_eq!(json["kept"], serde_json::json!(["claude-code"]));

    // A run that selects everything installed keeps nothing, so the mark is gone.
    let all = fixture.succeeds(&[
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
    ]);
    assert!(
        all.contains("runtimes: claude-code, codex  providers"),
        "{all}"
    );
    assert!(!all.contains("kept"), "{all}");
}

#[test]
fn init_says_to_reinstall_when_a_plugin_executable_is_not_beside_feld() {
    let fixture = Fixture::new();
    // `feld` alone, without the `feld-plugin-<name>` binaries `cargo build` puts beside it.
    let elsewhere = tempfile::tempdir().expect("create a directory for feld alone");
    let feld = elsewhere.path().join("feld");
    fs::copy(FELD, &feld).expect("copy feld");

    let output = Command::new(&feld)
        .arg("--home")
        .arg(fixture.home())
        .env("HOME", fixture.home())
        .env_remove("FELD_HOME")
        .args(["init", "--yes"])
        .output()
        .expect("run feld init");
    assert_eq!(output.status.code(), Some(1));
    let refusal = String::from_utf8_lossy(&output.stderr);
    assert!(
        refusal.contains("Error: the executable of the claude-code Plugin is not beside feld: ")
            && refusal.contains(
                "\nnext: reinstall Feld so that `feld` and `feld-plugin-claude-code` are in the same directory\n"
            ),
        "{refusal}"
    );
    assert!(
        !refusal.contains("make build") && !refusal.contains("cargo install"),
        "{refusal}"
    );
    assert!(
        !fixture.plugin_dir("runtime", "claude-code").exists(),
        "nothing is installed when a Plugin cannot be"
    );
}

/// The codex Runtime Adapter sits outside the default, so it is installed when the answer
/// names it, and the daemon then lists it beside claude-code.
#[test]
fn codex_is_installed_beside_claude_code_when_the_answer_names_it() {
    let fixture = Fixture::new();

    let chosen = fixture.json(&[
        "--json",
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
    ]);
    assert_eq!(
        chosen["chosen"],
        serde_json::json!(["claude-code", "codex", "tmux"])
    );
    let codex = fixture.plugin_dir("runtime", "codex");
    assert!(is_complete(&codex));
    assert!(codex.join("skills/feld/SKILL.md").is_file());
    for shim in ["hook", "queue-helper"] {
        assert!(
            is_runnable(&codex.join("bin").join(shim)),
            "the {shim} shim is runnable"
        );
    }

    fixture.json(&["--json", "daemon", "start"]);
    let status = fixture.json(&["--json", "daemon", "status"]);
    assert_eq!(names(&status["runtimes"]), ["claude-code", "codex"]);
    let human = fixture.succeeds(&["daemon", "status"]);
    assert!(human.contains("runtimes:   claude-code, codex"), "{human}");
}

/// The list a terminal answers: the cursor moves, `Space` checks, and `Enter` installs
/// what is checked.
#[test]
fn a_terminal_checks_the_plugins_with_the_cursor_and_space() {
    let fixture = Fixture::new();

    let mut terminal = fixture.on_a_terminal(&["init"]);
    terminal.waits_for("Choose the Plugins to install");
    // What is on PATH is checked, and the cursor starts on the first choice.
    terminal.waits_for("\u{276f} \u{25c9} claude-code");
    terminal.presses(b"\x1b[B");
    terminal.waits_for("\u{276f} \u{25cb} codex");
    terminal.presses(b" ");
    terminal.waits_for("\u{276f} \u{25c9} codex");
    // A held key or a fast hand lands as one read, and every press in it counts: down to
    // herdr, check it, confirm.
    terminal.presses(b"\x1b[B\x1b[B \r");
    // `feld init` runs each Plugin it has just installed for the first time, and macOS
    // checks a new executable on its first run: seconds apiece under load.
    terminal.declines_the_shell_line();
    terminal.waits_for("Start the daemon now?");
    terminal.presses(b"n");
    assert_eq!(terminal.ends(), Some(0));

    // The list leaves the screen, and the step folds into the Plugins it installed.
    let text = terminal.text();
    assert!(
        text.contains("\u{25c7} Plugins  runtimes: claude-code, codex  providers: herdr, tmux"),
        "{text}"
    );
    assert!(text.contains("\u{25c7} Daemon  not started"), "{text}");
    assert!(text.contains("next:  feld daemon start"), "{text}");
    assert!(is_complete(&fixture.plugin_dir("runtime", "codex")));
    assert!(is_complete(&fixture.plugin_dir("provider", "herdr")));

    // A re-run opens the list on what is installed, so a bare Enter keeps it.
    let mut again = fixture.on_a_terminal(&["init"]);
    again.waits_for("\u{276f} \u{25c9} claude-code");
    again.waits_for("\u{25c9} codex");
    again.presses(b"\r");
    again.declines_the_shell_line();
    again.waits_for("Start the daemon now?");
    again.presses(b"n");
    assert_eq!(again.ends(), Some(0));
    assert!(
        again.text().contains("runtimes: claude-code, codex"),
        "{}",
        again.text()
    );
}

/// A flag answers its own kind, so the list asks about the kind that is left.
#[test]
fn a_flag_answers_its_kind_and_the_list_asks_about_the_other() {
    let fixture = Fixture::new();

    let mut terminal = fixture.on_a_terminal(&["init", "--runtime", "codex"]);
    terminal.waits_for("Execution Provider");
    assert!(
        !terminal.text().contains("Runtime Adapter"),
        "{}",
        terminal.text()
    );
    terminal.presses(b"\r");
    terminal.declines_the_shell_line();
    terminal.waits_for("Start the daemon now?");
    terminal.presses(b"n");
    assert_eq!(terminal.ends(), Some(0));
    assert!(
        terminal.text().contains("runtimes: codex  providers: tmux"),
        "{}",
        terminal.text()
    );
}

/// The shell and daemon questions take their answers from the keys: a yes adds the
/// `feld env` line and starts the daemon, whose Web UI the end shows. A re-run finds the
/// line and asks nothing about it, and restarts the running daemon so that it reads the
/// Plugins.
#[test]
fn a_terminal_starts_the_daemon_and_then_restarts_it() {
    let fixture = Fixture::new();
    let zshrc = fixture.home().join(".zshrc");

    let mut terminal = fixture.on_a_terminal(&["init"]);
    terminal.waits_for("Choose the Plugins to install");
    terminal.presses(b"\r");
    // The ask names the file and the lines it adds.
    terminal.waits_for("Add this line to your shell settings?  \u{25c9} Yes  \u{25cb} No");
    let asked = terminal.text();
    assert!(
        asked.contains(&format!("\u{25c6} Shell  {}", zshrc.display())),
        "{asked}"
    );
    assert!(
        asked.contains("+ # Added by `feld init`") && asked.contains("+ eval \"$(feld env)\""),
        "{asked}"
    );
    terminal.presses(b"y");
    terminal.waits_for("Start the daemon now?  \u{25c9} Yes  \u{25cb} No");
    // The arrows move between the two, and Enter takes the one chosen.
    terminal.presses(b"\x1b[C");
    terminal.waits_for("Start the daemon now?  \u{25cb} Yes  \u{25c9} No");
    terminal.presses(b"\x1b[D\r");
    assert_eq!(terminal.ends(), Some(0), "{}", terminal.text());
    let text = terminal.text();
    assert!(
        text.contains(&format!(
            "\u{25c7} Shell  added eval \"$(feld env)\" to {}",
            zshrc.display()
        )),
        "{text}"
    );
    assert!(
        text.contains("\u{25c7} Daemon  started \u{b7} pid "),
        "{text}"
    );
    let status = fixture.json(&["--json", "daemon", "status"]);
    assert_eq!(status["state"], "running");
    let first = status["pid"].clone();
    assert!(
        text.contains(&format!(
            "\u{2514} \u{2713} Feld is ready.\r\n   next:  source {}\r\n   next:  feld ui\r\n   ui:    {}  \u{b7}  first time in this browser: run `feld ui` to sign in\r\n",
            zshrc.display(),
            status["webUi"].as_str().expect("the Web UI")
        )),
        "{text}"
    );
    assert_eq!(feld_env_lines(&zshrc), 1);

    let mut again = fixture.on_a_terminal(&["init"]);
    again.waits_for("Choose the Plugins to install");
    again.presses(b"\r");
    again.waits_for("Restart the daemon to load the Plugins?");
    assert!(!again.text().contains("Add this line"), "{}", again.text());
    again.presses(b"y");
    assert_eq!(again.ends(), Some(0), "{}", again.text());
    assert!(
        again
            .text()
            .contains("\u{25c7} Daemon  restarted \u{b7} pid "),
        "{}",
        again.text()
    );
    let status = fixture.json(&["--json", "daemon", "status"]);
    assert_ne!(status["pid"], first);
}

/// `NO_COLOR` draws the asks and the steps as plain text, without colors or symbols.
#[test]
fn no_color_draws_plain_text_on_a_terminal() {
    let fixture = Fixture::new();

    let mut terminal = fixture.on_a_terminal_with(&["init"], &[("NO_COLOR", "1")]);
    terminal.waits_for("> [x] claude-code");
    terminal.presses(b"\r");
    terminal.declines_the_shell_line();
    terminal.waits_for("Start the daemon now?  [x] Yes  [ ] No");
    terminal.presses(b"n");
    assert_eq!(terminal.ends(), Some(0));
    let drawn = terminal.drawn();
    assert!(
        !drawn.contains("\x1b[1;94m") && !drawn.contains("\x1b[38;5;242m"),
        "{drawn}"
    );
    for symbol in ['\u{223f}', '\u{25c7}', '\u{25c6}', '\u{2502}', '\u{2713}'] {
        assert!(!drawn.contains(symbol), "{symbol}: {drawn}");
    }
    let text = terminal.text();
    assert!(
        text.contains("Plugins:     runtimes: claude-code  providers: tmux\r\n"),
        "{text}"
    );
    assert!(text.contains("Daemon:      not started\r\n"), "{text}");
}

/// Leaving the list installs nothing and gives the terminal back.
/// `--yes` installs the shipped Plugins whose products are on PATH, and only those.
#[test]
fn yes_installs_the_plugins_whose_products_are_on_path() {
    for (on_path, runtimes, providers) in [
        (&["codex", "herdr"][..], &["codex"][..], &["herdr"][..]),
        (
            &["claude", "codex", "tmux", "herdr"][..],
            &["claude-code", "codex"][..],
            &["herdr", "tmux"][..],
        ),
    ] {
        let fixture = Fixture::new();
        fixture.with_on_path(on_path);
        let printed = fixture.succeeds(&["init", "--yes"]);
        assert!(printed.contains("Feld is ready."), "{printed}");
        assert!(!printed.contains("is not on PATH"), "{printed}");
        let listed = fixture.json(&["daemon", "status", "--json"]);
        assert_eq!(names(&listed["runtimes"]), runtimes, "{on_path:?}");
        assert_eq!(names(&listed["providers"]), providers, "{on_path:?}");
    }
}

/// With no Runtime on PATH, Claude Code's Adapter is installed all the same, and the end
/// says `claude` is missing.
#[test]
fn claude_code_stands_in_for_a_runtime_missing_from_path() {
    let fixture = Fixture::new();
    fixture.with_on_path(&["tmux"]);

    let printed = fixture.succeeds(&["init", "--yes"]);
    assert!(
        printed.contains("Plugins:     runtimes: claude-code  providers: tmux"),
        "{printed}"
    );
    let (_, end) = printed.split_once("Feld is ready.\n").expect("ready");
    assert!(
        end.contains("  note:  Claude Code: `claude` is not on PATH"),
        "{printed}"
    );

    // What is not a terminal answers the same without `--yes`.
    let other = Fixture::new();
    other.with_on_path(&["herdr"]);
    let result = other.json(&["init", "--json"]);
    assert_eq!(
        result["chosen"],
        json!(["claude-code", "herdr"]),
        "{result}"
    );
    assert_eq!(
        result["notes"]
            .to_string()
            .matches("is not on PATH")
            .count(),
        1,
        "{result}"
    );
}

/// With no Provider on PATH, none named and none installed, `feld init` places nothing and
/// says to install one, whether or not it would ask.
#[test]
fn init_stops_when_no_provider_is_on_path() {
    let fixture = Fixture::new();
    fixture.with_on_path(&["claude"]);

    for arguments in [&["init", "--yes"][..], &["init", "--json"][..]] {
        let output = fixture.feld(arguments);
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(said.contains("install tmux or herdr"), "{said}");
        let next = "install tmux or herdr so that it is on PATH, and run `feld init` again";
        if arguments.contains(&"--json") {
            let error: serde_json::Value =
                serde_json::from_str(said.lines().next().unwrap()).unwrap();
            assert_eq!(error["error"]["next"], next, "{said}");
        } else {
            assert!(
                said.contains(&format!(
                    "Error: no Execution Provider is on PATH\nnext: {next}\n"
                )),
                "{said}"
            );
        }
    }
    let mut terminal = fixture.on_a_terminal(&["init"]);
    assert_eq!(terminal.ends(), Some(1));
    let text = terminal.text();
    assert!(text.contains("install tmux or herdr"), "{text}");
    assert!(!text.contains("Choose the Plugins"), "{text}");
    assert!(!fixture.home().join("plugins").exists());

    // A Provider named on the command line, or one already installed, is answer enough.
    fixture.with_on_path(&[]);
    let flagged = fixture.json(&[
        "init",
        "--runtime",
        "codex",
        "--provider",
        "herdr",
        "--json",
    ]);
    assert_eq!(flagged["chosen"], json!(["codex", "herdr"]), "{flagged}");
    assert!(
        !flagged["notes"].to_string().contains("is not on PATH"),
        "{flagged}"
    );
    let again = fixture.json(&["init", "--json"]);
    assert_eq!(names(&again["providers"]), ["herdr"], "{again}");
}

/// A relative directory on PATH, an empty one among them, reads from the working directory,
/// as a shell finds a command there.
#[test]
fn a_relative_directory_on_path_reads_from_the_working_directory() {
    let fixture = Fixture::new();
    fixture.with_on_path(&["codex", "herdr"]);

    for (path, directory) in [
        ("path", fixture.home().to_path_buf()),
        (":", fixture.path()),
    ] {
        let output = Command::new(FELD)
            .arg("--home")
            .arg(fixture.home())
            .args(["init", "--json"])
            .current_dir(&directory)
            .env("HOME", fixture.home())
            .env("PATH", path)
            .env_remove("FELD_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .output()
            .expect("run feld");
        assert!(
            output.status.success(),
            "PATH={path}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).expect("parse feld JSON");
        assert_eq!(
            result["chosen"],
            json!(["codex", "herdr"]),
            "PATH={path}: {result}"
        );
    }
}

/// The list confirms only with a check in each kind; a kind left without one says so under
/// its heading and keeps the list up.
#[test]
fn the_list_wants_a_check_in_each_kind() {
    let fixture = Fixture::new();

    let mut terminal = fixture.on_a_terminal(&["init"]);
    terminal.waits_for("\u{276f} \u{25c9} claude-code");
    // Down to tmux and uncheck it, then try to confirm.
    terminal.presses(b"\x1b[B\x1b[B \r");
    terminal.waits_for("check at least one");
    let text = terminal.text();
    let lines: Vec<&str> = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    assert!(
        lines.windows(2).any(|pair| pair
            == [
                "\u{2502}  Execution Provider",
                "\u{2502}  check at least one"
            ]),
        "{text}"
    );
    assert_eq!(text.matches("check at least one").count(), 1, "{text}");
    // Check herdr instead, and the list confirms.
    terminal.presses(b"\x1b[B \r");
    terminal.declines_the_shell_line();
    terminal.waits_for("Start the daemon now?");
    terminal.presses(b"n");
    assert_eq!(terminal.ends(), Some(0));
    let text = terminal.text();
    assert!(
        text.contains("\u{25c7} Plugins  runtimes: claude-code  providers: herdr"),
        "{text}"
    );
}

#[test]
fn leaving_the_list_installs_nothing() {
    let fixture = Fixture::new();

    for leave in [&b"\x03"[..], &b"q"[..]] {
        let mut terminal = fixture.on_a_terminal(&["init"]);
        terminal.waits_for("\u{276f} \u{25c9} claude-code");
        terminal.presses(leave);
        assert_eq!(terminal.ends(), Some(1), "{}", terminal.text());
        let text = terminal.text();
        assert!(text.contains("no Plugin was installed"), "{text}");
        assert!(!text.contains("runtimes:"), "{text}");
        assert!(!fixture.plugin_dir("runtime", "claude-code").exists());
    }
}

/// A person's own `statusLine` and `otelHeadersHelper` change only with their consent:
/// without a terminal or `--yes` the run shows them and leaves them as they are, and a
/// terminal that says yes gets the status line chained behind Feld's helper, which
/// Claude Code then runs to one line of theirs and Feld's. A re-run asks nothing.
#[test]
fn init_chains_a_persons_status_line_with_consent_and_asks_nothing_again() {
    let fixture = Fixture::new();
    let claude = fixture.home().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let script = claude.join("statusline.sh");
    fs::write(&script, "#!/bin/sh\ncat > /dev/null\necho mine\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let settings_path = claude.join("settings.json");
    let theirs = json!({
        "model": "opus",
        "otelHeadersHelper": "/usr/local/bin/my-headers",
        "statusLine": {"type": "command", "command": script, "padding": 0}
    });
    fs::write(&settings_path, serde_json::to_vec(&theirs).unwrap()).unwrap();
    let settings =
        || -> Value { serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap() };
    let arguments = ["init", "--runtime", "claude-code", "--provider", "tmux"];

    // Without a terminal or `--yes`, nothing of theirs changes.
    let output = fixture.feld(&arguments);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    // The entries read as what each value is now (`-`) and what Feld writes (`+`).
    let helper = claude.join("skills/feld/bin/status-line");
    assert!(
        stdout.contains(&format!(
            "Claude Code: not prepared (needs your consent)\n  skills/feld/\n  + {}\n  otelHeadersHelper  the current value is kept in Feld's prepare record\n  - /usr/local/bin/my-headers\n  + '{}'\n  statusLine  the current command keeps running, chained behind Feld's\n  - {}\n  + '{}' --chain '{}'\n  run `feld init --yes` to apply them\nFeld is not ready.\n",
            claude.join("skills/feld").display(),
            claude.join("skills/feld/bin/otel-headers").display(),
            script.display(),
            helper.display(),
            script.display()
        )),
        "{stdout}"
    );
    assert_eq!(settings(), theirs);
    assert!(!claude.join("skills/feld").exists());

    // A terminal is asked, and a yes writes them.
    let mut terminal = fixture.on_a_terminal(&arguments);
    terminal.waits_for("Apply these changes to your Claude Code settings?");
    terminal.presses(b"y");
    terminal.declines_the_shell_line();
    terminal.waits_for("Start the daemon now?");
    terminal.presses(b"n");
    assert_eq!(terminal.ends(), Some(0), "{}", terminal.text());
    assert!(
        terminal.text().contains(
            "\u{25c7} Claude Code  prepared \u{b7} otelHeadersHelper replaced \u{b7} statusLine chained"
        ),
        "{}",
        terminal.text()
    );
    let prepared = settings();
    assert_eq!(prepared["model"], "opus");
    assert_eq!(prepared["statusLine"]["padding"], 0);
    let command = prepared["statusLine"]["command"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        command,
        format!("'{}' --chain '{}'", helper.display(), script.display())
    );
    assert_eq!(
        prepared["otelHeadersHelper"],
        format!(
            "'{}'",
            claude.join("skills/feld/bin/otel-headers").display()
        )
    );
    let record: Value = serde_json::from_slice(
        &fs::read(fixture.home().join("prepared/claude-code.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["roots"][claude.display().to_string()]["replaced"]["otelHeadersHelper"],
        "/usr/local/bin/my-headers"
    );

    // Claude Code runs the command with the status line JSON on stdin.
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", fixture.home())
        .env("FELD_HOME", fixture.home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"context_window":{"used_percentage":42}}"#)
        .unwrap();
    let line = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&line.stdout),
        "mine · Feld · context 42%\n"
    );

    // A re-run finds everything Feld's and asks nothing.
    let again = fixture.succeeds(&arguments);
    assert!(again.contains("Claude Code: prepared\n"), "{again}");
    assert!(
        !again.contains("chain") && !again.contains("not prepared"),
        "{again}"
    );
    assert_eq!(settings(), prepared);
}

/// A tmux server of the test's own, which the test takes down with it.
struct Tmux {
    socket: String,
}

impl Tmux {
    fn output(&self, arguments: &[&str]) -> Output {
        Command::new("tmux")
            .arg("-L")
            .arg(&self.socket)
            .arg("-f")
            .arg("/dev/null")
            .args(arguments)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .output()
            .expect("run tmux")
    }

    fn run(&self, arguments: &[&str]) -> String {
        let output = self.output(arguments);
        assert!(
            output.status.success(),
            "tmux {}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// The pane as it reads, without the blanks after each line and after the last one.
    fn screen(&self) -> String {
        let captured = self.run(&["capture-pane", "-p"]);
        let lines: Vec<&str> = captured.lines().map(str::trim_end).collect();
        lines.join("\n").trim_end().to_owned()
    }

    /// Waits for the pane to show `wanted`, as long as [`Terminal::waits_for`] waits.
    fn waits_for(&self, wanted: &str) {
        let until = Instant::now() + Duration::from_secs(60);
        while Instant::now() < until {
            if self.screen().contains(wanted) {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("the pane never showed {wanted:?}:\n{}", self.screen());
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .arg("-L")
            .arg(&self.socket)
            .arg("kill-server")
            .output();
    }
}

/// The number after `prefix` in `text` put as `stand_in`, so that a screen compares across
/// runs.
/// How many lines of `path` read `feld env`.
fn feld_env_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("feld env"))
        .count()
}

fn stand_in(text: &str, prefix: &str, stand_in: &str) -> String {
    match text.split_once(prefix) {
        Some((before, after)) => {
            let rest = after.trim_start_matches(|char: char| char.is_ascii_digit());
            format!("{before}{prefix}{stand_in}{rest}")
        }
        None => text.to_owned(),
    }
}

/// In a tmux pane narrower than 80 columns, whose symbols of ambiguous width take two
/// columns, the asks leave the screen as they are answered, and `feld init` ends on one
/// line for each step.
#[test]
fn init_in_a_narrow_tmux_pane_ends_on_one_line_for_each_step() {
    let fixture = Fixture::short();
    let home = fixture.home().display().to_string();
    let path = fixture.path().display().to_string();
    let tmux = Tmux {
        socket: format!("feld-init-{}", std::process::id()),
    };
    let script = fixture.home().join("init.sh");
    fs::write(
        &script,
        format!(
            "unset FELD_HOME FELD_API FELD_SESSION_TOKEN FELD_WORKSPACE FELD_ACTOR CLAUDE_CONFIG_DIR CODEX_HOME NO_COLOR CLICOLOR_FORCE\nunset ZDOTDIR\nHOME='{home}' PATH='{path}' SHELL=/bin/zsh LANG=en_US.UTF-8 '{FELD}' --home '{home}' init\necho \"exit $?\"\nsleep 60\n"
        ),
    )
    .expect("write the script the pane runs");
    // The symbols of the ten that East Asian Width calls ambiguous take two columns, which
    // `codepoint-widths` of tmux 3.5 and later sets. An earlier tmux draws them in one.
    let script = script.display().to_string();
    let session = ["new-session", "-d", "-x", "72", "-y", "30", "sh", &script];
    let widths = [
        "set",
        "-s",
        "codepoint-widths",
        "U+25C7=2,U+25C6=2,U+2502=2,U+2514=2,U+25CB=2",
    ];
    let with_widths = [&["start-server", ";"], &widths[..], &[";"], &session[..]].concat();
    if !tmux.output(&with_widths).status.success() {
        tmux.run(&session);
    }

    let wordmark = format!("\u{223f} Feld v{}", env!("CARGO_PKG_VERSION"));
    tmux.waits_for("Choose the Plugins to install");
    assert_eq!(
        tmux.screen().replace(&home, "<home>"),
        [
            wordmark.as_str(),
            "",
            "\u{25c7} Feld home  <home>",
            "\u{2502}",
            "\u{25c6} Choose the Plugins to install  \u{2191}\u{2193} move \u{b7} space check \u{b7} enter confirm",
            "\u{2502}",
            "\u{2502}  Runtime Adapter",
            "\u{2502}  \u{276f} \u{25c9} claude-code   Claude Code",
            "\u{2502}    \u{25cb} codex         Codex",
            "\u{2502}",
            "\u{2502}  Execution Provider",
            "\u{2502}    \u{25c9} tmux          tmux",
            "\u{2502}    \u{25cb} herdr         herdr",
            "\u{2514}",
        ]
        .join("\n")
    );
    tmux.run(&["send-keys", "Enter"]);
    tmux.waits_for("Add this line to your shell settings?");
    tmux.run(&["send-keys", "Enter"]);
    tmux.waits_for("Start the daemon now?");
    tmux.run(&["send-keys", "Enter"]);
    tmux.waits_for("exit 0");

    let screen = stand_in(&tmux.screen(), "pid ", "<pid>");
    let screen = stand_in(&screen, "http://127.0.0.1:", "<port>");
    let screen = stand_in(&screen, "ui:    http://127.0.0.1:", "<port>");
    assert_eq!(
        screen.replace(&home, "<home>"),
        [
            wordmark.as_str(),
            "",
            "\u{25c7} Feld home  <home>",
            "\u{25c7} Plugins  runtimes: claude-code  providers: tmux",
            "\u{25c7} Claude Code  prepared",
            "\u{25c7} Shell  added eval \"$(feld env)\" to <home>/.zshrc",
            "\u{25c7} Daemon  started \u{b7} pid <pid> \u{b7} http://127.0.0.1:<port>",
            "\u{2502}",
            "\u{2514} \u{2713} Feld is ready.",
            "   next:  source <home>/.zshrc",
            "   next:  feld ui",
            "   ui:    http://127.0.0.1:<port>/ui/  \u{b7}  first time in this browser: run",
            " `feld ui` to sign in",
            "   note:  Claude Code: run /reload-plugins in a Claude Code that is alre",
            "ady open",
            "   note:  Claude Code: restart a Claude Code started before this init to",
            " send its metrics",
            "   note:  run `feld doctor` if something does not work",
            "exit 0",
        ]
        .join("\n")
    );
}

/// `feld uninstall` is the reverse of `feld init`: without a terminal or `--yes` it lists
/// what goes and removes nothing, and with `--yes` the person's `statusLine` and
/// `otelHeadersHelper` come back, the Feld plugin and skill and the Feld hooks go, and the
/// Feld home gives up its settings while its records stay.
#[test]
fn uninstall_gives_back_what_init_replaced_and_keeps_the_records() {
    let fixture = Fixture::new();
    let claude = fixture.home().join(".claude");
    let codex = fixture.home().join(".codex");
    fs::create_dir_all(&claude).unwrap();
    fs::create_dir_all(&codex).unwrap();
    let settings_path = claude.join("settings.json");
    let theirs = json!({
        "model": "opus",
        "otelHeadersHelper": "/usr/local/bin/my-headers",
        "statusLine": {"type": "command", "command": "~/it's mine.sh", "padding": 0}
    });
    fs::write(&settings_path, serde_json::to_vec(&theirs).unwrap()).unwrap();
    let hooks_path = codex.join("hooks.json");
    let their_hooks = json!({"hooks": {"Stop": [{"matcher": "", "hooks": [{"type": "command", "command": "/mine.sh"}]}]}});
    fs::write(&hooks_path, serde_json::to_vec(&their_hooks).unwrap()).unwrap();
    let read = |path: &Path| -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() };
    fixture.succeeds(&[
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
        "--yes",
    ]);
    let prepared = read(&settings_path);
    assert_ne!(prepared, theirs);
    fs::create_dir_all(fixture.home().join("sessions/session_1")).unwrap();
    for n in 1..=4 {
        fs::write(fixture.home().join(format!("daemon.log.{n}")), "rotated").unwrap();
    }

    // Without a terminal or `--yes`, everything is listed and nothing goes.
    let output = fixture.feld(&["uninstall"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("otelHeadersHelper    restore   to: /usr/local/bin/my-headers")
            && stdout.contains("statusLine           unchain   to: ~/it's mine.sh")
            && stdout.contains("hooks                remove    8 Feld hook entries")
            && stdout.contains("run `feld uninstall --yes`"),
        "{stdout}"
    );
    assert_eq!(read(&settings_path), prepared);
    assert!(claude.join("skills/feld").is_dir());
    assert!(fixture.home().join("plugins").is_dir());

    let printed = fixture.succeeds(&["uninstall", "--yes"]);
    assert!(printed.contains("Uninstalled"), "{printed}");
    assert_eq!(read(&settings_path), theirs);
    assert_eq!(read(&hooks_path), their_hooks);
    assert!(!claude.join("skills/feld").exists());
    assert!(!codex.join("skills/feld").exists());
    assert!(!fixture.home().join("plugins").exists());
    assert!(!fixture.home().join("prepared").exists());
    assert!(!fixture.home().join("daemon.log").exists());
    for n in 1..=4 {
        assert!(!fixture.home().join(format!("daemon.log.{n}")).exists());
    }
    assert!(fixture.home().join("sessions/session_1").is_dir());
}

/// What a person put among Feld's files stays: `feld init` writes into a `skills/feld/`
/// holding files of theirs only with their consent, and `feld uninstall` leaves such a
/// directory and a person's hook in a Feld hook group where they are.
#[test]
fn init_and_uninstall_leave_what_a_person_put_among_feld_s_files() {
    let fixture = Fixture::new();
    let claude = fixture.home().join(".claude");
    let codex = fixture.home().join(".codex");
    let their_skill = codex.join("skills/feld");
    fs::create_dir_all(&their_skill).unwrap();
    fs::write(their_skill.join("SKILL.md"), "mine").unwrap();
    fs::write(their_skill.join("notes.md"), "mine").unwrap();
    let init = [
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
    ];

    let output = fixture.feld(&init);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains(
            "skills/feld/  files of other names stay; Feld's are written over those of its names"
        ),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(their_skill.join("SKILL.md")).unwrap(),
        "mine"
    );

    fixture.succeeds(&[&init[..], &["--yes"]].concat());
    assert_ne!(
        fs::read_to_string(their_skill.join("SKILL.md")).unwrap(),
        "mine"
    );
    assert!(their_skill.join("notes.md").is_file());

    fs::write(claude.join("skills/feld/bin/mine"), "mine").unwrap();
    let hooks_path = codex.join("hooks.json");
    let mut hooks: Value = serde_json::from_slice(&fs::read(&hooks_path).unwrap()).unwrap();
    let mine = json!({"type": "command", "command": "/mine.sh"});
    hooks["hooks"]["Stop"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(mine.clone());
    fs::write(&hooks_path, serde_json::to_vec(&hooks).unwrap()).unwrap();
    // A person's additions leave the preparation as it is.
    fixture.succeeds(&init);

    let output = fixture.feld(&["uninstall"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("skills/feld/         keep      current: bin/mine")
            && stdout.contains("skills/feld/         keep      current: notes.md"),
        "{stdout}"
    );
    fixture.succeeds(&["uninstall", "--yes"]);
    assert!(claude.join("skills/feld/bin/mine").is_file());
    assert!(their_skill.join("notes.md").is_file());
    let left: Value = serde_json::from_slice(&fs::read(&hooks_path).unwrap()).unwrap();
    assert_eq!(
        left,
        json!({"hooks": {"Stop": [{"matcher": "", "hooks": [mine]}]}})
    );
}

/// A file of Feld's name is Feld's only as Feld wrote it: `feld init` asks before writing
/// over a person's own `skills/feld/SKILL.md`, and `feld uninstall` keeps one a person
/// edited after `feld init`.
#[test]
fn a_person_s_file_of_feld_s_name_is_theirs() {
    let fixture = Fixture::new();
    let claude = fixture
        .home()
        .join(".claude/skills/feld/skills/feld/SKILL.md");
    let codex = fixture.home().join(".codex/skills/feld/SKILL.md");
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::write(&codex, "mine").unwrap();
    let init = [
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
    ];

    let output = fixture.feld(&init);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("Codex:       not prepared (needs your consent)"),
        "{stdout}"
    );
    assert_eq!(fs::read_to_string(&codex).unwrap(), "mine");

    fixture.succeeds(&[&init[..], &["--yes"]].concat());
    fs::write(&claude, "edited").unwrap();
    fs::write(&codex, "edited").unwrap();
    let output = fixture.feld(&["uninstall"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("skills/feld/         keep      current: skills/feld/SKILL.md")
            && stdout.contains("skills/feld/         keep      current: SKILL.md"),
        "{stdout}"
    );
    fixture.succeeds(&["uninstall", "--yes"]);
    assert_eq!(fs::read_to_string(&claude).unwrap(), "edited");
    assert_eq!(fs::read_to_string(&codex).unwrap(), "edited");
}

/// The telemetry environment an earlier Feld wrote into the Claude Code `env` goes at
/// `feld init` only as a whole, and `feld uninstall` gives it back. A person's own collector,
/// and a loopback endpoint without the rest, stay as they are.
#[test]
fn init_takes_out_only_the_whole_telemetry_environment_an_earlier_feld_wrote() {
    let read = |path: &Path| -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() };
    let mut earlier: serde_json::Map<String, Value> =
        feld_core::telemetry::shell_exporter_environment("http://127.0.0.1:4318")
            .into_iter()
            .map(|(key, value)| (key, Value::String(value)))
            .collect();
    let mut own = earlier.clone();
    own.insert(
        "OTEL_EXPORTER_OTLP_ENDPOINT".to_owned(),
        json!("https://collector.example.com"),
    );
    let part = json!({"OTEL_EXPORTER_OTLP_ENDPOINT": "http://127.0.0.1:4318"});
    for env in [Value::Object(own), part] {
        let fixture = Fixture::new();
        let claude = fixture.home().join(".claude");
        fs::create_dir_all(&claude).unwrap();
        let settings_path = claude.join("settings.json");
        fs::write(
            &settings_path,
            serde_json::to_vec(&json!({"env": env})).unwrap(),
        )
        .unwrap();
        fixture.succeeds(&[
            "init",
            "--runtime",
            "claude-code",
            "--provider",
            "tmux",
            "--yes",
        ]);
        assert_eq!(read(&settings_path)["env"], env);
    }

    let fixture = Fixture::new();
    let claude = fixture.home().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let settings_path = claude.join("settings.json");
    earlier.insert("MINE".to_owned(), json!("1"));
    let theirs = json!({"model": "opus", "env": earlier});
    fs::write(&settings_path, serde_json::to_vec(&theirs).unwrap()).unwrap();
    let asked = fixture.feld(&["init", "--runtime", "claude-code", "--provider", "tmux"]);
    let stdout = String::from_utf8_lossy(&asked.stdout);
    assert!(
        stdout.contains("env  the current value is kept in Feld's prepare record")
            && stdout.contains("+ {\"MINE\":\"1\"}"),
        "{stdout}"
    );
    assert_eq!(read(&settings_path), theirs);

    fixture.succeeds(&[
        "init",
        "--runtime",
        "claude-code",
        "--provider",
        "tmux",
        "--yes",
    ]);
    assert_eq!(read(&settings_path)["env"], json!({"MINE": "1"}));
    let uninstall = fixture.feld(&["uninstall"]);
    assert!(
        String::from_utf8_lossy(&uninstall.stdout).contains("env                  restore"),
        "{uninstall:?}"
    );
    fixture.succeeds(&["uninstall", "--yes"]);
    let restored = read(&settings_path);
    assert_eq!(restored["env"], theirs["env"]);
    assert_eq!(restored["model"], "opus");
}

/// `feld init` and `feld uninstall` keep the order of the keys a person wrote in their
/// settings, at the top and inside, as the files themselves list them.
#[test]
fn init_and_uninstall_keep_the_order_of_a_persons_keys() {
    let fixture = Fixture::new();
    let claude = fixture.home().join(".claude");
    let codex = fixture.home().join(".codex");
    fs::create_dir_all(&claude).unwrap();
    fs::create_dir_all(&codex).unwrap();
    let settings_path = claude.join("settings.json");
    fs::write(
        &settings_path,
        r#"{"theme": "dark", "permissions": {"deny": [], "allow": ["Bash(ls)"]}, "model": "opus", "env": {"ZETA": "1", "ALPHA": "2"}}"#,
    )
    .unwrap();
    let hooks_path = codex.join("hooks.json");
    fs::write(
        &hooks_path,
        r#"{"zeta": 1, "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "/mine.sh"}], "matcher": ""}]}, "alpha": 2}"#,
    )
    .unwrap();
    let theirs =
        |path: &Path| -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() };
    let before = (theirs(&settings_path), theirs(&hooks_path));
    let keys =
        |value: &Value| -> Vec<String> { value.as_object().unwrap().keys().cloned().collect() };
    let keeps_the_order = |settings: &Value, hooks: &Value| {
        assert_eq!(
            keys(settings)[..4],
            ["theme", "permissions", "model", "env"],
            "{settings}"
        );
        assert_eq!(keys(&settings["permissions"]), ["deny", "allow"]);
        assert_eq!(keys(&settings["env"]), ["ZETA", "ALPHA"]);
        assert_eq!(keys(hooks), ["zeta", "hooks", "alpha"], "{hooks}");
        assert_eq!(keys(&hooks["hooks"])[0], "Stop");
        assert_eq!(keys(&hooks["hooks"]["Stop"][0]), ["hooks", "matcher"]);
        assert_eq!(
            keys(&hooks["hooks"]["Stop"][0]["hooks"][0]),
            ["type", "command"]
        );
    };

    fixture.succeeds(&[
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
        "--yes",
    ]);
    let (settings, hooks) = (theirs(&settings_path), theirs(&hooks_path));
    assert_ne!((&settings, &hooks), (&before.0, &before.1));
    keeps_the_order(&settings, &hooks);

    fixture.succeeds(&["uninstall", "--yes"]);
    let (settings, hooks) = (theirs(&settings_path), theirs(&hooks_path));
    assert_eq!((&settings, &hooks), (&before.0, &before.1));
    assert_eq!(keys(&settings), ["theme", "permissions", "model", "env"]);
    keeps_the_order(&settings, &hooks);
}

/// A prepare that stops before the record or the settings are written leaves the settings
/// as they are; the next `feld init` completes it, and an uninstall gives back the values
/// of the person's that it replaced.
#[test]
fn a_prepare_stopped_before_its_writes_is_completed_by_the_next_init() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let claude = fixture.home().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let settings_path = claude.join("settings.json");
    let theirs = json!({
        "otelHeadersHelper": "/original/helper",
        "statusLine": {"type": "command", "command": "/original/status"}
    });
    fs::write(&settings_path, serde_json::to_vec(&theirs).unwrap()).unwrap();
    let read = |path: &Path| -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() };
    let init = [
        "--json",
        "init",
        "--runtime",
        "claude-code",
        "--provider",
        "tmux",
        "--yes",
    ];

    // A dangling symlink reads as no record and refuses the directory the record goes in.
    let prepared = fixture.home().join("prepared");
    std::os::unix::fs::symlink(fixture.home().join("missing"), &prepared).unwrap();
    let failed = fixture.feld(&init);
    assert!(!failed.status.success(), "{failed:?}");
    assert_eq!(read(&settings_path), theirs);
    fs::remove_file(&prepared).unwrap();

    // The record is saved, and the directory of the settings refuses their new file.
    fs::create_dir_all(claude.join("skills")).unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = fixture.feld(&init);
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!failed.status.success(), "{failed:?}");
    assert_eq!(read(&settings_path), theirs);

    let again = fixture.feld(&init);
    assert!(again.status.success(), "{again:?}");
    let settings = read(&settings_path);
    let helper = claude.join("skills/feld/bin/otel-headers");
    assert_eq!(
        settings["otelHeadersHelper"],
        format!("'{}'", helper.display())
    );
    let status = settings["statusLine"]["command"].as_str().unwrap();
    assert!(status.ends_with("--chain '/original/status'"), "{status}");

    fixture.succeeds(&["uninstall", "--yes"]);
    assert_eq!(read(&settings_path), theirs);
}

/// A Codex prepared under one `CODEX_HOME` is given back by an uninstall run under another:
/// the prepare record names the root `feld init` wrote into.
#[test]
fn uninstall_gives_back_the_codex_root_the_prepare_record_names() {
    let fixture = Fixture::new();
    let prepared_root = fixture.home().join("codex-a");
    let other_root = fixture.home().join("codex-b");
    fs::create_dir_all(&prepared_root).unwrap();
    let hooks_path = prepared_root.join("hooks.json");
    let their_hooks = json!({"hooks": {"Stop": [{"matcher": "", "hooks": [{"type": "command", "command": "/mine.sh"}]}]}});
    fs::write(&hooks_path, serde_json::to_vec(&their_hooks).unwrap()).unwrap();
    // A table a person wrote without writable_roots: the key the spool brings goes with it.
    let config_path = prepared_root.join("config.toml");
    let their_config =
        "# mine\nmodel = \"o3\"\n\n[sandbox_workspace_write]\nnetwork_access = true # keep\n";
    fs::write(&config_path, their_config).unwrap();
    let read = |path: &Path| -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() };
    let init = fixture.feld_with(
        &[
            "--json",
            "init",
            "--runtime",
            "codex",
            "--provider",
            "tmux",
            "--yes",
        ],
        &[("CODEX_HOME", prepared_root.to_str().unwrap())],
    );
    assert!(init.status.success(), "{init:?}");
    assert!(prepared_root.join("skills/feld").is_dir());
    let spool = fs::canonicalize(fixture.home())
        .unwrap()
        .join("spool/shared");
    assert_eq!(
        fs::read_to_string(&config_path).unwrap(),
        format!("{their_config}writable_roots = [\"{}\"]\n", spool.display())
    );
    let record = read(&fixture.home().join("prepared/codex.json"));
    assert_eq!(
        record["roots"],
        json!([prepared_root.display().to_string()])
    );
    assert_eq!(
        record["spool"],
        json!({prepared_root.display().to_string(): {"path": spool.display().to_string(), "added": "key"}})
    );
    assert_eq!(
        record["skills"][prepared_root.display().to_string()]
            .as_object()
            .map(|files| files.keys().cloned().collect::<Vec<_>>()),
        Some(vec![
            "SKILL.md".to_owned(),
            "bin/hook".to_owned(),
            "bin/queue-helper".to_owned()
        ])
    );

    let uninstall = fixture.feld_with(
        &["uninstall", "--yes"],
        &[("CODEX_HOME", other_root.to_str().unwrap())],
    );
    assert!(uninstall.status.success(), "{uninstall:?}");
    assert!(!prepared_root.join("skills/feld").exists());
    assert_eq!(read(&hooks_path), their_hooks);
    assert_eq!(fs::read_to_string(&config_path).unwrap(), their_config);
}

/// The preparation in a Runtime's configuration root is shared by every Feld home. A Feld
/// home whose `feld init` found it in place, or did not prepare that Runtime, keeps no
/// prepare record, and its uninstall warns before consent that taking the preparation out
/// stops the attach of the other Feld homes too.
#[test]
fn uninstall_warns_when_this_home_did_not_prepare_the_runtime() {
    let fixture = Fixture::new();
    fixture.succeeds(&[
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
        "--yes",
    ]);
    let other = fixture.home().join("other-feld-home");
    let init = fixture.feld_at(
        &other,
        &[
            "init",
            "--runtime",
            "claude-code",
            "--provider",
            "tmux",
            "--yes",
        ],
        &[],
    );
    let output = fixture.feld_at(&other, &["uninstall"], &[]);
    let listed = fixture.feld_at(&other, &["uninstall", "--json"], &[]);
    let _ = fixture.feld_at(&other, &["daemon", "stop"], &[]);
    assert!(init.status.success(), "{init:?}");
    assert!(!other.join("prepared/claude-code.json").exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    let warning = "warning: `feld init` of this Feld home did not put these in place";
    assert!(
        stdout.contains("skills/feld/         remove") && stdout.contains(warning),
        "{stdout}"
    );
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed["applied"], false, "{listed}");
    assert_eq!(listed["runtimes"][0]["name"], "claude-code", "{listed}");
    assert_eq!(listed["runtimes"][0]["prepared"], false, "{listed}");

    // The Feld home that did prepare them lists them without the warning.
    let output = fixture.feld(&["uninstall"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("skills/feld/") && !stdout.contains(warning),
        "{stdout}"
    );
}

/// A prepare record that does not read, cut short or in another shape, stops `feld init`
/// before the Runtime's settings or the record change, and the error names the record.
#[test]
fn init_stops_at_a_prepare_record_that_does_not_read() {
    let fixture = Fixture::new();
    let arguments = [
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
        "--yes",
    ];
    fixture.succeeds(&arguments);
    let settings = [
        fixture.home().join(".claude/settings.json"),
        fixture.home().join(".codex/config.toml"),
        fixture.home().join(".codex/hooks.json"),
    ];
    let prepared: Vec<Vec<u8>> = settings
        .iter()
        .map(|path| fs::read(path).unwrap())
        .collect();
    let records = fixture.home().join("prepared");
    let (claude_code, codex) = (records.join("claude-code.json"), records.join("codex.json"));
    let whole = fs::read(&claude_code).unwrap();
    for (record, contents) in [
        (&claude_code, whole[..whole.len() / 2].to_vec()),
        (&claude_code, b"null".to_vec()),
        (&claude_code, b"{}".to_vec()),
        (&codex, br#"{"roots": {"not": "a list"}}"#.to_vec()),
        (&codex, b"{}".to_vec()),
    ] {
        let kept = fs::read(record).unwrap();
        fs::write(record, &contents).unwrap();
        let output = fixture.feld(&arguments);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains(&format!(
                "the prepare record {} does not read",
                record.display()
            )) && stderr.contains("fix it or move it aside"),
            "{stderr}"
        );
        for (path, before) in settings.iter().zip(&prepared) {
            assert_eq!(&fs::read(path).unwrap(), before, "{}", path.display());
        }
        assert_eq!(fs::read(record).unwrap(), contents);
        fs::write(record, kept).unwrap();
    }
}

/// A prepare record that does not read, cut short or in another shape, stops `feld
/// uninstall` before anything changes, and the error names the record.
#[test]
fn uninstall_stops_at_a_prepare_record_that_does_not_read() {
    let fixture = Fixture::new();
    fixture.succeeds(&[
        "init",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
        "--yes",
    ]);
    let settings_path = fixture.home().join(".claude/settings.json");
    let prepared = fs::read(&settings_path).unwrap();
    let records = fixture.home().join("prepared");
    let (claude_code, codex) = (records.join("claude-code.json"), records.join("codex.json"));
    let whole = fs::read(&claude_code).unwrap();
    for (record, contents) in [
        (&claude_code, whole[..whole.len() / 2].to_vec()),
        (&claude_code, b"null".to_vec()),
        (&claude_code, b"{}".to_vec()),
        (&codex, br#"{"roots": {"not": "a list"}}"#.to_vec()),
        (&codex, b"{}".to_vec()),
    ] {
        let kept = fs::read(record).unwrap();
        fs::write(record, &contents).unwrap();
        let output = fixture.feld(&["uninstall", "--yes"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains(&format!(
                "the prepare record {} does not read",
                record.display()
            )) && stderr.contains("fix it or move it aside"),
            "{stderr}"
        );
        assert_eq!(fs::read(&settings_path).unwrap(), prepared);
        assert_eq!(fs::read(record).unwrap(), contents);
        assert!(fixture.home().join(".claude/skills/feld").is_dir());
        fs::write(record, kept).unwrap();
    }
}

/// `--purge` takes the records as well, and what in the Feld home is not Feld's stays.
#[test]
fn uninstall_purge_removes_the_records_and_leaves_what_is_not_fields() {
    let fixture = Fixture::new();
    fixture.succeeds(&["init", "--yes"]);
    fs::create_dir_all(fixture.home().join("sessions/session_1")).unwrap();
    fs::write(fixture.home().join("notes.txt"), "mine").unwrap();

    let result = fixture.json(&["uninstall", "--purge", "--yes", "--json"]);
    assert_eq!(result["applied"], true, "{result}");
    assert!(
        result["remove"]
            .as_array()
            .unwrap()
            .contains(&json!("sessions/")),
        "{result}"
    );
    assert!(!fixture.home().join("sessions").exists());
    assert!(!fixture.home().join("plugins").exists());
    assert!(!fixture.home().join(".claude/skills/feld").exists());
    assert_eq!(
        fs::read_to_string(fixture.home().join("notes.txt")).unwrap(),
        "mine"
    );
}

/// A Feld home a daemon ran in, which leaves its spool there, goes as a whole with
/// `--purge`.
#[test]
fn uninstall_purge_removes_a_home_a_daemon_ran_in() {
    let fixture = Fixture::new();
    let home = fixture.home().join("feld-home");
    let init = fixture.feld_at(&home, &["init", "--yes"], &[]);
    assert!(init.status.success(), "{init:?}");
    let started = fixture.feld_at(&home, &["daemon", "start"], &[]);
    assert!(started.status.success(), "{started:?}");
    assert!(home.join("spool").is_dir());

    let output = fixture.feld_at(&home, &["uninstall", "--purge", "--yes"], &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("feld home:    removed"), "{stdout}");
    assert!(!home.exists(), "{stdout}");
}

/// The socket a daemon of a home too long for one left in a temporary directory goes with
/// the home's other settings: the one its record names, under the temporary directory the
/// daemon ran with, and the one under the temporary directory of the uninstall.
#[test]
fn uninstall_removes_the_sockets_a_long_home_left_in_temporary_directories() {
    let fixture = Fixture::new();
    fixture.succeeds(&["init", "--yes"]);
    let _ = fixture.feld(&["daemon", "stop"]);
    let short = feld_core::home::FeldHome::resolve(Some(fixture.home().to_path_buf()))
        .unwrap()
        .short_socket_path();
    let relative = short
        .strip_prefix(short.parent().unwrap().parent().unwrap())
        .unwrap();
    let (daemon_temp, uninstall_temp) = (fixture.home().join("ta"), fixture.home().join("tb"));
    let sockets = [daemon_temp.join(relative), uninstall_temp.join(relative)];
    let _listeners = sockets.each_ref().map(|socket| {
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        std::os::unix::net::UnixListener::bind(socket).unwrap()
    });
    // What a daemon that died leaves: its record, which names where it listened.
    fs::write(
        fixture.home().join("daemon.json"),
        json!({
            "instanceId": "instance_1",
            "pid": u32::MAX,
            "api": "http://127.0.0.1:9",
            "socket": sockets[0],
        })
        .to_string(),
    )
    .unwrap();

    let output = fixture.feld_with(
        &["uninstall", "--yes", "--json"],
        &[("TMPDIR", uninstall_temp.to_str().unwrap())],
    );
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    for socket in &sockets {
        assert!(
            result["remove"]
                .as_array()
                .unwrap()
                .contains(&json!(socket.display().to_string())),
            "{result}"
        );
        assert!(!socket.parent().unwrap().exists(), "{result}");
    }
}

/// A directory with none of the marks of a Feld home loses nothing to `uninstall --yes`,
/// and a file of the same name that is not the one Feld writes is no mark.
#[test]
fn uninstall_refuses_a_directory_that_is_not_a_feld_home() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let fixture = Fixture::new();
    fixture.succeeds(&["init", "--yes"]);
    let _ = fixture.feld(&["daemon", "stop"]);
    fn owner_only(path: &Path) {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    // Each plants, in the directory, a file of a mark's name that Feld did not write there.
    type Plant = fn(&Path, &Path);
    let not_marks: [(&str, Plant); 6] = [
        ("none", |_, _| {}),
        ("a lock of another tool", |repo, _| {
            fs::write(repo.join("daemon.lock"), "another tool lock").unwrap();
            owner_only(&repo.join("daemon.lock"));
        }),
        ("a lock others may read", |repo, _| {
            fs::write(repo.join("daemon.lock"), "").unwrap();
            fs::set_permissions(repo.join("daemon.lock"), fs::Permissions::from_mode(0o644))
                .unwrap();
        }),
        ("a store that is not SQLite", |repo, _| {
            fs::write(repo.join("feld.db"), "not a database").unwrap();
            owner_only(&repo.join("feld.db"));
        }),
        ("a token directory", |repo, _| {
            fs::create_dir(repo.join("api-token")).unwrap();
            owner_only(&repo.join("api-token"));
        }),
        ("a link to a Feld home's lock", |repo, feld_home| {
            symlink(feld_home.join("daemon.lock"), repo.join("daemon.lock")).unwrap();
        }),
    ];
    for (case, plant) in not_marks {
        let repo = fixture.home().join("repo");
        let _ = fs::remove_dir_all(&repo);
        for kept in ["plugins", "tokens", "sessions"] {
            fs::create_dir_all(repo.join(kept)).unwrap();
            fs::write(repo.join(kept).join("keep.txt"), "mine").unwrap();
        }
        plant(&repo, fixture.home());
        for args in [
            &["uninstall", "--yes"][..],
            &["uninstall", "--purge", "--yes"],
        ] {
            let output = fixture.feld_at(&repo, args, &[]);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(1), "{case}: {stderr}");
            assert!(stderr.contains("is not a Feld home"), "{case}: {stderr}");
        }
        for kept in ["plugins", "tokens", "sessions"] {
            assert!(repo.join(kept).join("keep.txt").is_file(), "{case}: {kept}");
        }
    }
}

/// A terminal is asked, and anything but yes removes nothing.
#[test]
fn a_terminal_is_asked_before_uninstall_removes_anything() {
    let fixture = Fixture::new();
    fixture.succeeds(&["init", "--yes"]);

    let mut terminal = fixture.on_a_terminal(&["uninstall"]);
    terminal.waits_for("Remove these? [y/N]");
    terminal.presses(b"n\r");
    assert_eq!(terminal.ends(), Some(1), "{}", terminal.drawn());
    assert!(fixture.home().join("plugins").is_dir());

    let mut terminal = fixture.on_a_terminal(&["uninstall"]);
    terminal.waits_for("Remove these? [y/N]");
    terminal.presses(b"y\r");
    assert_eq!(terminal.ends(), Some(0), "{}", terminal.drawn());
    assert!(!fixture.home().join("plugins").exists());
    assert!(!fixture.home().join(".claude/skills/feld").exists());
}

/// With a daemon running a Job, `uninstall --yes` kills the Session and stops the daemon
/// before it takes anything out, and the store it keeps reads again after the next
/// `feld init`: the Session ended.
#[test]
fn uninstall_kills_the_running_sessions_and_stops_the_daemon() {
    let fixture = Fixture::new();
    let socket = fixture.home().join("tmux.sock");
    let socket = socket.to_string_lossy().into_owned();
    let tmux_server = |arguments: &[&str]| {
        Command::new("tmux")
            .arg("-S")
            .arg(&socket)
            .args(arguments)
            .output()
            .expect("run tmux")
    };
    fixture.succeeds(&["init", "--runtime", "claude-code", "--provider", "tmux"]);
    // The Session runs on the machine's tmux, which the daemon finds on the test's `PATH`.
    let path = std::env::var("PATH").unwrap_or_default();
    let started = fixture.feld_with(&["daemon", "start"], &[("PATH", &path)]);
    assert!(started.status.success(), "{started:?}");
    let created = fixture.json(&[
        "actor",
        "create",
        "hello",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        &socket,
        "--json",
        "--",
        "sleep",
        "300",
    ]);
    let session = created["session"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("a started Session: {created}"))
        .to_owned();

    // The list without consent kills nothing and says so.
    let output = fixture.feld(&["uninstall", "--json"]);
    let planned: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(planned["applied"], false, "{planned}");
    assert_eq!(planned["daemon"]["sessions"][0]["kill"], true, "{planned}");
    assert_eq!(
        planned["daemon"]["sessions"][0]["killed"], false,
        "{planned}"
    );

    let result = fixture.json(&["uninstall", "--yes", "--json"]);
    let listed = &result["daemon"]["sessions"];
    let left = tmux_server(&["list-sessions"]);
    let _ = tmux_server(&["kill-server"]);
    assert!(
        !left.status.success(),
        "{}",
        String::from_utf8_lossy(&left.stdout)
    );
    assert_eq!(result["applied"], true, "{result}");
    assert_eq!(listed[0]["id"], session.as_str(), "{result}");
    assert_eq!(listed[0]["killed"], true, "{result}");
    assert_eq!(result["daemon"]["left"], json!([]), "{result}");
    assert_eq!(fixture.feld(&["daemon", "status"]).status.code(), Some(1));

    fixture.succeeds(&["init", "--yes"]);
    fixture.succeeds(&["daemon", "start"]);
    let read = fixture.json(&["session", "get", &session, "--json"]);
    assert!(!read["session"]["end"].is_null(), "{read}");
}

/// With the daemon stopped and a Session still open in its store, `uninstall --yes`
/// changes nothing and names the Session and the daemon start, the store still at the top
/// of the home as an earlier version kept it; after the start, it kills the Session.
#[test]
fn uninstall_stops_at_the_open_sessions_of_a_stopped_daemon() {
    let fixture = Fixture::new();
    let socket = fixture.home().join("tmux.sock");
    let socket = socket.to_string_lossy().into_owned();
    let tmux_server = |arguments: &[&str]| {
        Command::new("tmux")
            .arg("-S")
            .arg(&socket)
            .args(arguments)
            .output()
            .expect("run tmux")
    };
    fixture.succeeds(&["init", "--runtime", "claude-code", "--provider", "tmux"]);
    let path = std::env::var("PATH").unwrap_or_default();
    let started = fixture.feld_with(&["daemon", "start"], &[("PATH", &path)]);
    assert!(started.status.success(), "{started:?}");
    let created = fixture.json(&[
        "actor",
        "create",
        "hello",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        &socket,
        "--json",
        "--",
        "sleep",
        "300",
    ]);
    let session = created["session"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("a started Session: {created}"))
        .to_owned();
    fixture.succeeds(&["daemon", "stop"]);
    for name in ["feld.db", "feld.db-wal", "feld.db-shm"] {
        let store = fixture.home().join("admin").join(name);
        if store.exists() {
            fs::rename(&store, fixture.home().join(name)).unwrap();
        }
    }

    let output = fixture.feld(&["uninstall", "--yes"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains(&session) && stderr.contains("run `feld daemon start`"),
        "{stderr}"
    );
    assert!(fixture.home().join("feld.db").is_file());
    assert!(fixture.home().join("plugins").is_dir());
    assert!(fixture.home().join(".claude/skills/feld").is_dir());
    assert!(tmux_server(&["list-sessions"]).status.success());

    let started = fixture.feld_with(&["daemon", "start"], &[("PATH", &path)]);
    assert!(started.status.success(), "{started:?}");
    let result = fixture.json(&["uninstall", "--yes", "--json"]);
    let left = tmux_server(&["list-sessions"]);
    let _ = tmux_server(&["kill-server"]);
    assert_eq!(
        result["daemon"]["sessions"][0]["id"],
        session.as_str(),
        "{result}"
    );
    assert_eq!(result["daemon"]["left"], json!([]), "{result}");
    assert!(!left.status.success());
}
