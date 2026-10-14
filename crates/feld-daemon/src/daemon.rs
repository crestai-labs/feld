use std::fs::{self, File};
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::Deserialize;
use tokio::net::{TcpListener, UnixListener};
use tokio::sync::{Notify, watch};

use crate::api::{self, AllowedHosts, AppState, DaemonStatus, RestoredCounts};
use crate::control::SessionController;
use crate::scheduler::Scheduler;
use crate::service::{
    ActorService, BrowserService, MessageService, SessionService, TokenService, WorkspaceService,
};
use crate::store::{CLOSED_SESSION_TELEMETRY_RETENTION, Store};
use feld_core::home::{DaemonRecord, FeldHome, absent_as_none, open_lock_file, read_daemon_record};
use feld_core::id::{fresh_id, fresh_token};
use feld_core::plugin::{Plugins, Registry};
use feld_core::time;

const START_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the daemon removes the Telemetries of Sessions closed past their retention.
const TELEMETRY_RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How long the daemon holding the lock has to answer its status before it counts as
/// unresponsive.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
pub struct ShutdownAccepted {
    pub accepted: bool,
}

/// A process holds the daemon lock but does not answer its status within
/// [`STATUS_TIMEOUT`].
#[derive(Debug, thiserror::Error)]
#[error(
    "the Feld daemon recorded in {record} (pid {pid}, {api}) holds the daemon lock but did \
     not answer within {} seconds; stop it with `kill {pid}` and start it again with \
     `feld daemon start`",
    STATUS_TIMEOUT.as_secs()
)]
pub struct Unresponsive {
    pub pid: u32,
    pub api: String,
    pub record: String,
}

/// A Runtime's sandbox denied reading the local token in `admin/`, which [`probe`] asks the
/// daemon's status with.
#[derive(Debug, thiserror::Error)]
#[error("a sandbox denies reading the daemon local token")]
pub struct LocalTokenDenied;

#[derive(Debug)]
pub enum Presence {
    Running(Box<DaemonStatus>),
    NotRunning,
}

/// Whether a daemon runs for the Feld home. It only reads the Feld home, so a caller that
/// may not write there, such as a Runtime's sandbox, asks it too: a Feld home or a daemon
/// lock that does not exist answers that no daemon runs.
pub async fn probe(home: &FeldHome) -> Result<Presence> {
    probe_with(
        home,
        |record| async move { get_status(home, &record.api).await },
    )
    .await
}

/// [`probe`] with the status read by `fetch`, which a CLI inside a Runtime's sandbox reads
/// through the spool.
pub async fn probe_with<F, Fut>(home: &FeldHome, fetch: F) -> Result<Presence>
where
    F: FnOnce(DaemonRecord) -> Fut,
    Fut: Future<Output = Result<DaemonStatus>>,
{
    if !lock_held(home)? {
        return Ok(Presence::NotRunning);
    }
    let record = read_daemon_record(home)?;
    let status = tokio::time::timeout(STATUS_TIMEOUT, fetch(record.clone()))
        .await
        .map_err(|_| Unresponsive {
            pid: record.pid,
            api: record.api.clone(),
            record: home.record_path().display().to_string(),
        })??;
    if status.instance_id != record.instance_id
        || status.pid != record.pid
        || status.api != record.api
    {
        bail!("the process holding the daemon lock did not match its daemon record");
    }
    Ok(Presence::Running(Box::new(status)))
}

/// Whether a process holds the daemon lock of the Feld home. It only reads the Feld home.
pub fn lock_held(home: &FeldHome) -> Result<bool> {
    let Some(lock) = absent_as_none(File::open(home.lock_path()))
        .with_context(|| format!("open daemon lock {}", home.lock_path().display()))?
    else {
        return Ok(false);
    };
    match lock.try_lock_exclusive() {
        Ok(()) => {
            lock.unlock().context("unlock daemon lock")?;
            Ok(false)
        }
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(true),
        Err(error) => Err(error).context("inspect daemon lock"),
    }
}

/// Starts a daemon unless one runs. `port` is the API port to listen on, and without one
/// the daemon listens on the port [`bind_listener`] chooses. `hosts` are the host names
/// the daemon answers besides the loopback names.
pub async fn start(
    home: &FeldHome,
    port: Option<u16>,
    hosts: &[String],
) -> Result<(DaemonStatus, bool)> {
    let _operation_lock = lock_lifecycle(home)?;
    start_locked(home, None, port, None, hosts).await
}

/// Starts a daemon unless one runs. `local_token` is the local token the daemon keeps, and
/// without one it issues a new one. `carried` is the port of the daemon a restart stopped,
/// which the daemon started listens on without keeping it for the later starts.
async fn start_locked(
    home: &FeldHome,
    local_token: Option<&str>,
    port: Option<u16>,
    carried: Option<u16>,
    hosts: &[String],
) -> Result<(DaemonStatus, bool)> {
    // A daemon that died without unwinding leaves its lock held by the Collector it
    // started, for as long as that Collector takes to notice its parent is gone, and its
    // record naming an API that answers nothing. Starting waits for that to settle, so the
    // lock it reads tells the truth about a daemon that answers.
    let settle = Instant::now() + START_TIMEOUT;
    loop {
        match probe(home).await {
            Ok(Presence::Running(status)) => {
                ensure_listening_on(&status, port)?;
                ensure_answering(&status, hosts)?;
                keep_port(home, port)?;
                return Ok((*status, true));
            }
            Ok(Presence::NotRunning) => break,
            Err(error) if Instant::now() >= settle || error.is::<Unresponsive>() => {
                return Err(error);
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(25)).await,
        }
    }

    let port = named_port(home, port);
    let carried = carried.filter(|_| port.is_none());
    if let Some(port) = port.or(carried).or_else(|| default_port(home)) {
        drop(bind_port(port)?);
    }
    let executable = std::env::current_exe().context("locate feld executable")?;
    let log = home.open_owner_file(&home.log_path(), true)?;
    let stderr = log.try_clone().context("clone daemon log handle")?;
    let mut command = feld_core::process::command(executable);
    command
        .arg("--home")
        .arg(home.root())
        .arg("daemon")
        .arg("run")
        .stdin(if local_token.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    if local_token.is_some() {
        command.arg("--local-token-stdin");
    }
    if let Some(port) = port {
        command.arg("--port").arg(port.to_string());
    }
    if let Some(port) = carried {
        command.arg("--carried-port").arg(port.to_string());
    }
    for host in hosts {
        command.arg("--allow-host").arg(host);
    }
    let mut child = command.spawn().context("start daemon process")?;
    if let Some(token) = local_token {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("daemon stdin is piped");
        stdin
            .write_all(token.as_bytes())
            .context("hand the local token to the daemon")?;
    }
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        let child_status = child.try_wait().context("inspect starting daemon")?;
        match probe(home).await {
            Ok(Presence::Running(status)) => return Ok((*status, false)),
            Ok(Presence::NotRunning) if child_status.is_some() => {
                bail!(
                    "daemon exited during startup with {}; see {}",
                    child_status.expect("status was present"),
                    home.log_path().display()
                );
            }
            Ok(Presence::NotRunning) | Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok(Presence::NotRunning) | Err(_) => {
                bail!(
                    "daemon did not become ready within 10 seconds; see {}",
                    home.log_path().display()
                );
            }
        }
    }
}

pub async fn stop(home: &FeldHome) -> Result<bool> {
    // A Feld home that does not exist has no daemon to stop, and stays absent.
    if !home.root().exists() {
        return Ok(false);
    }
    let _operation_lock = lock_lifecycle(home)?;
    stop_locked(home).await
}

async fn stop_locked(home: &FeldHome) -> Result<bool> {
    let status = match probe(home).await? {
        Presence::Running(status) => *status,
        Presence::NotRunning => return Ok(false),
    };
    let token = home.read_token()?;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/daemon/shutdown", status.api))
        .bearer_auth(token)
        .send()
        .await;
    let shutdown_error = match response {
        Ok(response) => {
            if !response.status().is_success() {
                bail!("daemon rejected shutdown ({})", response.status());
            }
            match response.json::<ShutdownAccepted>().await {
                Ok(accepted) if accepted.accepted => None,
                Ok(_) => bail!("daemon did not accept shutdown"),
                Err(error) => Some(anyhow::Error::new(error).context("read shutdown response")),
            }
        }
        Err(error) => Some(anyhow::Error::new(error).context("request daemon shutdown")),
    };
    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        match probe(home).await {
            Ok(Presence::NotRunning) => return Ok(true),
            Ok(Presence::Running(current)) if current.instance_id != status.instance_id => {
                return Ok(true);
            }
            Ok(Presence::Running(_)) | Err(_) => {}
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if let Some(error) = shutdown_error {
        return Err(error)
            .context("the shutdown response failed and the same daemon instance remained running");
    }
    bail!("daemon did not stop within 10 seconds")
}

/// Whether a daemon was running to stop, and the status of the one started. The daemon
/// started keeps the local token of the one stopped, so a Web UI page that holds it reads
/// on.
/// `port` is the API port the started daemon listens on, and without one it listens on the
/// port [`bind_listener`] chooses. The started daemon answers the host names of the one
/// stopped and `hosts`.
pub async fn restart(
    home: &FeldHome,
    port: Option<u16>,
    hosts: &[String],
) -> Result<(bool, DaemonStatus)> {
    let _operation_lock = lock_lifecycle(home)?;
    let (running, mut answered) = match probe(home).await {
        Ok(Presence::Running(status)) => (api_port(&status.api), status.allowed_hosts),
        Err(error) if error.is::<Unresponsive>() => return Err(error),
        _ => (None, Vec::new()),
    };
    for host in hosts {
        if !answered.contains(host) {
            answered.push(host.clone());
        }
    }
    // Without a named port the default home keeps the port of the daemon it stops, so the
    // open Sessions, which reach the API they were started with, keep reaching it. A start
    // after a stop moves it to the default port.
    let carried = running.filter(|_| home.is_default());
    // A port that another process holds fails the restart before the running daemon stops.
    if let Some(port) = named_port(home, port).or(carried)
        && running.is_some()
        && running != Some(port)
    {
        drop(bind_port(port)?);
    }
    let local_token = home.read_token().ok();
    let stopped = stop_locked(home).await?;
    let local_token = local_token.filter(|_| stopped);
    let carried = carried.filter(|_| stopped);
    let (status, _) = start_locked(home, local_token.as_deref(), port, carried, &answered).await?;
    Ok((stopped, status))
}

/// Restarts the daemon from a process of its own, as `feld daemon restart` does, so the
/// daemon that asks can answer before it is stopped. `restarting` is cleared when that
/// process ends, so a restart that failed and left this daemon running can be asked again.
pub fn spawn_restart(home: &FeldHome, restarting: Arc<AtomicBool>) -> Result<()> {
    let executable = std::env::current_exe().context("locate feld executable")?;
    // The restart writes only its errors to the log, after this line; the daemon it starts
    // logs its own start.
    let log = home.open_owner_file(&home.log_path(), true)?;
    eprintln!("Feld daemon: restarting with `feld daemon restart`");
    let mut command = feld_core::process::command(executable);
    command
        .arg("--home")
        .arg(home.root())
        .arg("daemon")
        .arg("restart")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().context("start daemon restart")?;
    std::thread::spawn(move || {
        let _ = child.wait();
        restarting.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Serves the daemon until it is shut down. `local_token` is the local token handed over
/// by a restart, and without one the daemon issues a new one. `port` is the API port named
/// with `--port`, and without one the daemon listens on the port [`bind_listener`] chooses.
/// `carried` is the port of the daemon a restart stopped, which this daemon listens on
/// without keeping it for the later starts. `hosts` are the host names the daemon answers
/// besides the loopback names.
pub async fn run(
    home: FeldHome,
    version: &str,
    local_token: Option<String>,
    port: Option<u16>,
    carried: Option<u16>,
    hosts: Vec<String>,
) -> Result<()> {
    // The instant this process began serving, before the store, the Collector and the
    // recovery of open Sessions are set up.
    let started_at = time::now();
    home.create()?;
    let lock = open_lock(&home)?;
    lock.try_lock_exclusive().map_err(|error| {
        if error.kind() == ErrorKind::WouldBlock {
            anyhow::anyhow!("a daemon already owns {}", home.root().display())
        } else {
            anyhow::Error::from(error).context("lock Feld home")
        }
    })?;
    home.migrate_admin()?;
    crate::log::keep_rotated(&home)?;
    home.create_owner_dir(&home.spool_dir())?;

    let canonical_home = home.canonical_root()?;
    secure_store_files(&home)?;
    let store = Arc::new(Store::open(&home.store_path())?);
    let listeners = bind_listener(&home, port.or(carried)).await?;
    let (socket, socket_path) = bind_socket(&home)?;
    let address = listeners[0]
        .local_addr()
        .context("read daemon API address")?;
    let api_url = format!("http://{address}");
    let instance_id = fresh_id("daemon");
    if let Ok(previous_token) = fs::read_to_string(home.token_path()) {
        store.remember_token_secret("daemon-local", previous_token.trim())?;
    }
    let token = local_token.unwrap_or_else(fresh_token);
    store.remember_token_secret("daemon-local", &token)?;
    let internal_token = fresh_token();
    store.remember_token_secret("collector-internal", &internal_token)?;
    let collector = match crate::collector::Collector::start(&home, &api_url, &internal_token).await
    {
        Ok(collector) => Some(collector),
        Err(error) => {
            eprintln!("Feld telemetry Collector is unavailable: {error:#}");
            None
        }
    };
    let collector_endpoint = collector
        .as_ref()
        .map(|collector| collector.endpoint().to_owned());
    let collector_state = Arc::new(RwLock::new(collector_endpoint.clone()));
    // The Plugins this daemon can call. The shipped ones are first brought to this
    // version, and `feld daemon restart` is what makes a Plugin added or changed since
    // the last start take effect.
    match crate::bundled::executables_directory() {
        Ok(beside) => {
            for line in crate::bundled::update_installed(&home, &beside) {
                eprintln!("Feld Plugin: {line}");
            }
        }
        Err(error) => {
            eprintln!("Feld Plugin: the shipped Plugins are left as they were: {error:#}")
        }
    }
    let registry = Arc::new(Registry::load(&home, Some(&api_url)));
    for problem in registry.problems() {
        eprintln!("Feld Plugin: {problem}");
    }
    let controller = Arc::new(SessionController::with_collector_state(
        Arc::clone(&store),
        Arc::clone(&registry) as Arc<dyn Plugins>,
        home.clone(),
        api_url.clone(),
        Arc::clone(&collector_state),
    ));
    let recovered = controller
        .recover()
        .await
        .context("recover open Sessions")?;
    for problem in &recovered {
        eprintln!("Feld observe: {problem}");
    }
    let timezone = feld_core::time::local_timezone().unwrap_or_else(|| "UTC".to_owned());
    let counts = store.counts()?;
    let status = DaemonStatus {
        state: "running".to_owned(),
        version: version.to_owned(),
        instance_id: instance_id.clone(),
        pid: std::process::id(),
        home: canonical_home.display().to_string(),
        api: api_url.clone(),
        started_at,
        server_time: String::new(),
        history_start: None,
        timezone,
        runtimes: registry.runtimes(),
        providers: registry.providers(),
        nodes: vec!["local".to_owned()],
        web_ui: Some(format!("{api_url}/ui/")),
        allowed_hosts: hosts.clone(),
        otlp: collector_endpoint,
        restored: RestoredCounts {
            actors: counts.actors,
            running_sessions: counts.running_sessions,
            schedules: counts.schedules,
        },
        scheduler: Default::default(),
    };
    let record = DaemonRecord {
        instance_id,
        pid: status.pid,
        api: api_url,
        port,
        socket: (socket_path != home.socket_path()).then(|| socket_path.clone()),
        environment: DaemonRecord::recorded_environment(),
    };
    home.write_owner_file(&home.token_path(), token.as_bytes())?;
    home.write_owner_file(&home.record_path(), &serde_json::to_vec(&record)?)?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let schedule_notify = Arc::new(Notify::new());
    let scheduler = Scheduler::new(
        Arc::clone(&store),
        Arc::clone(&controller),
        Arc::clone(&schedule_notify),
    );
    let scheduler_status = scheduler.status();
    let started = time::parse(&status.started_at).context("parse daemon start")?;
    let state = AppState {
        token,
        internal_token,
        workspaces: Arc::new(WorkspaceService::new(Arc::clone(&store))),
        actors: Arc::new(ActorService::new(
            Arc::clone(&store),
            Arc::clone(&registry),
            status.timezone.clone(),
            schedule_notify,
            started,
        )),
        messages: Arc::new(MessageService::new(Arc::clone(&store), shutdown_tx.clone())),
        sessions: Arc::new(SessionService::new(
            Arc::clone(&store),
            Arc::clone(&registry),
        )),
        tokens: Arc::new(TokenService::new(Arc::clone(&store))),
        browsers: Arc::new(BrowserService::new(Arc::clone(&store), address.port())),
        hosts: Arc::new(AllowedHosts::new(address.port(), hosts)),
        status,
        registry,
        collector: Arc::clone(&collector_state),
        controller: Arc::clone(&controller),
        scheduler: scheduler_status,
        shutdown: shutdown_tx.clone(),
        restarting: Arc::new(AtomicBool::new(false)),
        home: home.clone(),
    };
    let observer = tokio::spawn(controller.run_periodic(recovered, shutdown_rx.clone()));
    let scheduler_task = tokio::spawn(scheduler.supervise(shutdown_rx.clone()));
    let collector_task = tokio::spawn(supervise_collector(
        home.clone(),
        record.api.clone(),
        state.internal_token.clone(),
        collector,
        Arc::clone(&collector_state),
        shutdown_rx.clone(),
    ));
    let spool_task = tokio::spawn(api::serve_spool(state.clone(), shutdown_rx.clone()));
    let retention_task = tokio::spawn(retain_telemetries(Arc::clone(&store), shutdown_rx.clone()));
    // Each loopback address and the socket are served until shutdown, and one that ends
    // ends the others.
    let router = api::router(state);
    let mut servers: Vec<_> = listeners
        .into_iter()
        .map(|listener| serve(listener, router.clone(), &shutdown_tx))
        .collect();
    servers.push(serve(socket, router, &shutdown_tx));
    let mut result = Ok(());
    for server in servers {
        let served = server
            .await
            .context("join daemon API")
            .and_then(|served| served.context("serve daemon API"));
        result = result.and(served);
    }
    let _ = shutdown_tx.send(true);
    // The observation this daemon is in the middle of runs to its end before the lock is
    // released. An observation calls its Provider and writes what came back, so a reading
    // left running past the lock would write what it saw of a Session the next daemon has
    // already taken over, and close it on a reading older than that daemon's own.
    observer.abort();
    let _ = observer.await;
    let _ = scheduler_task.await;
    let _ = collector_task.await;
    let _ = spool_task.await;
    let _ = retention_task.await;

    remove_if_exists(&home.token_path())?;
    remove_if_exists(&socket_path)?;
    // `feld daemon stop` takes the released lock for a stopped daemon, so the store is
    // closed first, and the next process to open it finds no WAL this one left.
    store.close()?;
    lock.unlock().context("unlock Feld home")?;
    result
}

/// Removes the Telemetries of Sessions closed past their retention and those over the
/// Session limit, now and every interval until shutdown.
async fn retain_telemetries(store: Arc<Store>, mut shutdown: watch::Receiver<bool>) {
    let mut interval = tokio::time::interval(TELEMETRY_RETENTION_INTERVAL);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let store = Arc::clone(&store);
                let closed_before = time::format(chrono::Utc::now() - CLOSED_SESSION_TELEMETRY_RETENTION);
                let pruned = tokio::task::spawn_blocking(move || store.prune_telemetries(&closed_before)).await;
                match pruned {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => eprintln!("Feld telemetry retention failed: {error:#}"),
                    Err(error) => eprintln!("Feld telemetry retention failed: {error}"),
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

async fn supervise_collector(
    home: FeldHome,
    daemon_api: String,
    internal_token: String,
    mut collector: Option<crate::collector::Collector>,
    status: Arc<RwLock<Option<String>>>,
    mut shutdown: watch::Receiver<bool>,
) {
    const RESTART_DELAYS: [Duration; 5] = [
        Duration::from_millis(100),
        Duration::from_millis(250),
        Duration::from_millis(500),
        Duration::from_secs(1),
        Duration::from_secs(2),
    ];
    let mut failures = 0_usize;
    loop {
        if *shutdown.borrow() {
            break;
        }
        if let Some(active) = collector.as_mut() {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                result = active.wait_for_exit() => {
                    match result {
                        Ok(exit) => eprintln!("Feld telemetry Collector exited with {exit}"),
                        Err(error) => eprintln!("Feld telemetry Collector supervision failed: {error:#}"),
                    }
                    *status.write().expect("Collector status lock poisoned") = None;
                    collector = None;
                    failures = 0;
                }
            }
            continue;
        }

        let delay = RESTART_DELAYS[failures.min(RESTART_DELAYS.len() - 1)];
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            () = tokio::time::sleep(delay) => {}
        }
        if *shutdown.borrow() {
            break;
        }
        match crate::collector::Collector::start(&home, &daemon_api, &internal_token).await {
            Ok(restarted) => {
                *status.write().expect("Collector status lock poisoned") =
                    Some(restarted.endpoint().to_owned());
                collector = Some(restarted);
                failures = 0;
            }
            Err(error) => {
                *status.write().expect("Collector status lock poisoned") = None;
                failures = failures.saturating_add(1);
                eprintln!("Feld telemetry Collector restart failed: {error:#}");
            }
        }
    }
    *status.write().expect("Collector status lock poisoned") = None;
    if let Some(collector) = collector {
        collector.shutdown().await;
    }
}

/// Serves the API on `listener` until shutdown, and shuts the daemon down when it ends.
fn serve<L>(
    listener: L,
    router: axum::Router,
    shutdown_tx: &watch::Sender<bool>,
) -> tokio::task::JoinHandle<std::io::Result<()>>
where
    L: axum::serve::Listener,
    L::Addr: std::fmt::Debug,
{
    let mut shutdown_rx = shutdown_tx.subscribe();
    let shutdown_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        let served = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                while !*shutdown_rx.borrow() {
                    if shutdown_rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;
        let _ = shutdown_tx.send(true);
        served
    })
}

/// Listens on the socket of the Feld home, the one place the parts beside a Runtime send
/// their Session token, and answers where. A home whose path is too long for a socket has
/// it in a directory only this user may write. A socket a daemon that died left is
/// replaced: the lock this daemon holds says no other serves the home.
fn bind_socket(home: &FeldHome) -> Result<(UnixListener, PathBuf)> {
    let path = home.socket_path();
    let listener = match bind_owner_socket(&path) {
        Err(error) if error.kind() == ErrorKind::InvalidInput => {
            let path = home.short_socket_path();
            secure_socket_dir(path.parent().expect("the socket sits in a directory"))?;
            let listener = bind_owner_socket(&path)
                .with_context(|| format!("listen on {}", path.display()))?;
            return serve_socket(listener).map(|listener| (listener, path));
        }
        bound => bound.with_context(|| format!("listen on {}", path.display()))?,
    };
    serve_socket(listener).map(|listener| (listener, path))
}

fn bind_owner_socket(path: &Path) -> std::io::Result<std::os::unix::net::UnixListener> {
    absent_as_none(fs::remove_file(path))?;
    let listener = std::os::unix::net::UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn serve_socket(listener: std::os::unix::net::UnixListener) -> Result<UnixListener> {
    listener
        .set_nonblocking(true)
        .context("prepare daemon socket API")?;
    UnixListener::from_std(listener).context("serve daemon socket API")
}

/// Makes `directory` this user's own, and fails when another user holds it: no one else
/// may place a socket where the parts beside a Runtime send their Session token.
fn secure_socket_dir(directory: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    match fs::DirBuilder::new().mode(0o700).create(directory) {
        Err(error) if error.kind() != ErrorKind::AlreadyExists => {
            return Err(error).with_context(|| format!("create {}", directory.display()));
        }
        _ => {}
    }
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("inspect {}", directory.display()))?;
    // SAFETY: geteuid has no preconditions and cannot fail.
    let user = unsafe { libc::geteuid() };
    if !metadata.is_dir() || metadata.uid() != user || metadata.mode() & 0o077 != 0 {
        bail!(
            "{} is not a directory only this user may use; remove it and start the daemon again",
            directory.display()
        );
    }
    Ok(())
}

/// The API port of a Feld home, in this order: the port named with `--port`, the port
/// named before and kept in `daemon.json`, and [`DEFAULT_API_PORT`] for the default home.
/// Each fails the start while another process holds it, so the API stays where a Session
/// or a relay such as `tailscale serve` reaches it. Another home, as a test or an isolated
/// daemon uses, listens on the port it last listened on, else on one the OS chooses.
async fn bind_listener(home: &FeldHome, port: Option<u16>) -> Result<Vec<TcpListener>> {
    let listeners = if let Some(port) = port.or_else(|| default_port(home)) {
        bind_port(port)?
    } else {
        let preferred = read_daemon_record(home)
            .ok()
            .and_then(|record| api_port(&record.api));
        match preferred.and_then(|port| bind_loopback(port).ok()) {
            Some(listeners) => listeners,
            None => bind_any_port()?,
        }
    };
    listeners
        .into_iter()
        .map(|listener| {
            listener
                .set_nonblocking(true)
                .context("prepare daemon loopback API")?;
            TcpListener::from_std(listener).context("serve daemon loopback API")
        })
        .collect()
}

/// Listens on `port` of the loopback addresses, and fails with a reason a person reads when
/// another process holds it. A process listening on every address holds the loopback port
/// too, and the bind alone misses it because of SO_REUSEADDR, so that is tried first.
fn bind_port(port: u16) -> Result<Vec<std::net::TcpListener>> {
    for (wildcard, loopback) in [
        (
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        ),
        (
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ),
    ] {
        match std::net::TcpListener::bind(SocketAddr::new(wildcard, port)) {
            Err(error) if error.kind() == ErrorKind::AddrInUse => {
                return Err(anyhow::Error::new(PortInUse {
                    port,
                    address: loopback,
                }));
            }
            // A host without the address family leaves only the others to check.
            Ok(_) | Err(_) => {}
        }
    }
    bind_loopback(port)
}

/// Listens on a port the OS chooses, on each loopback address.
fn bind_any_port() -> Result<Vec<std::net::TcpListener>> {
    // The port the OS chooses for `127.0.0.1` can be held on `[::1]`, so another is tried.
    let mut attempts = 16;
    loop {
        match bind_loopback(0) {
            Err(error) if error.is::<PortInUse>() && attempts > 1 => attempts -= 1,
            result => return result,
        }
    }
}

/// Listens on `port` of `127.0.0.1`, and of `[::1]` on a host with IPv6, as `localhost`
/// names either. Port 0 takes the port the OS chooses for `127.0.0.1` on both.
fn bind_loopback(port: u16) -> Result<Vec<std::net::TcpListener>> {
    let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let v4 = std::net::TcpListener::bind(v4).map_err(|error| listen_error(error, v4))?;
    let port = v4.local_addr().context("read daemon API address")?.port();
    let v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port);
    match std::net::TcpListener::bind(v6) {
        Ok(v6) => Ok(vec![v4, v6]),
        Err(error) if without_ipv6(&error) => Ok(vec![v4]),
        Err(error) => Err(listen_error(error, v6)),
    }
}

fn listen_error(error: std::io::Error, address: SocketAddr) -> anyhow::Error {
    if error.kind() == ErrorKind::AddrInUse {
        anyhow::Error::new(PortInUse {
            port: address.port(),
            address: address.ip(),
        })
    } else {
        anyhow::Error::from(error).context(format!("listen on {address}"))
    }
}

/// Whether listening failed because the host has no IPv6 loopback.
fn without_ipv6(error: &std::io::Error) -> bool {
    error.kind() == ErrorKind::AddrNotAvailable
        || matches!(
            error.raw_os_error(),
            Some(libc::EAFNOSUPPORT | libc::EPROTONOSUPPORT | libc::EADDRNOTAVAIL)
        )
}

/// The port a daemon started for the Feld home listens on: the one named before and kept in
/// `daemon.json`, else the default port of the default home. Another home takes whatever
/// port is free, so it has none.
pub fn start_port(home: &FeldHome) -> Option<u16> {
    named_port(home, None).or_else(|| default_port(home))
}

/// Whether a daemon could listen on `port` now: false while another process holds it, and
/// an error when listening fails for another reason, as in a sandbox that denies it.
pub fn port_free(port: u16) -> Result<bool> {
    match bind_port(port) {
        Ok(_) => Ok(true),
        Err(error) if error.is::<PortInUse>() => Ok(false),
        Err(error) => Err(error),
    }
}

/// Another process holds the port a daemon is to listen on.
#[derive(Debug)]
struct PortInUse {
    port: u16,
    address: IpAddr,
}

impl std::fmt::Display for PortInUse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "port {} is in use on {}; stop the process that holds it or choose another port with --port",
            self.port, self.address
        )
    }
}

impl std::error::Error for PortInUse {}

/// The API port the default Feld home listens on unless a port is named.
const DEFAULT_API_PORT: u16 = 7381;

/// The port named with `--port`, else the one named before and kept in `daemon.json`.
fn named_port(home: &FeldHome, port: Option<u16>) -> Option<u16> {
    port.or_else(|| read_daemon_record(home).ok().and_then(|record| record.port))
}

fn default_port(home: &FeldHome) -> Option<u16> {
    home.is_default().then_some(DEFAULT_API_PORT)
}

fn api_port(api: &str) -> Option<u16> {
    api.strip_prefix("http://")?
        .parse::<SocketAddr>()
        .ok()
        .map(|address| address.port())
}

/// A daemon already running answers a start that names another port with the way to move it.
fn ensure_listening_on(status: &DaemonStatus, port: Option<u16>) -> Result<()> {
    match port {
        Some(port) if api_port(&status.api) != Some(port) => bail!(
            "the daemon already listens on {}; run `feld daemon restart --port {port}` to move it",
            status.api
        ),
        _ => Ok(()),
    }
}

/// A daemon already running answers a start that names a host it does not answer with the
/// way to add it.
fn ensure_answering(status: &DaemonStatus, hosts: &[String]) -> Result<()> {
    match hosts
        .iter()
        .find(|host| !status.allowed_hosts.contains(host))
    {
        Some(host) => bail!(
            "the running daemon does not answer {host}; run `feld daemon restart --allow-host {host}` to add it"
        ),
        None => Ok(()),
    }
}

/// Keeps `port` for the later starts of a daemon that already listens on it.
fn keep_port(home: &FeldHome, port: Option<u16>) -> Result<()> {
    let Some(port) = port else {
        return Ok(());
    };
    let mut record = read_daemon_record(home)?;
    if record.port != Some(port) {
        record.port = Some(port);
        home.write_owner_file(&home.record_path(), &serde_json::to_vec(&record)?)?;
    }
    Ok(())
}

pub async fn get_status(home: &FeldHome, api: &str) -> Result<DaemonStatus> {
    let token = home.read_token().map_err(|error| {
        let denied = error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == ErrorKind::PermissionDenied)
        });
        if denied {
            error.context(LocalTokenDenied)
        } else {
            error
        }
    })?;
    get_status_as(api, &token).await
}

/// The status of the daemon at `api`, asked with `token`: the local token or a Session token.
pub async fn get_status_as(api: &str, token: &str) -> Result<DaemonStatus> {
    reqwest::Client::new()
        .get(format!("{api}/v1/daemon/status"))
        .bearer_auth(token)
        .send()
        .await
        .context("connect to daemon")?
        .error_for_status()
        .context("daemon rejected status request")?
        .json()
        .await
        .context("read daemon status")
}

fn open_lock(home: &FeldHome) -> Result<File> {
    open_lock_file(&home.lock_path(), "daemon lock")
}

fn lock_lifecycle(home: &FeldHome) -> Result<File> {
    home.create()?;
    let file = open_lock_file(&home.lifecycle_lock_path(), "daemon lifecycle lock")?;
    file.lock_exclusive().context("lock daemon lifecycle")?;
    Ok(file)
}

/// Makes the store owner-only before SQLite opens it, so the `-wal` and `-shm` files
/// SQLite creates take the same mode, and secures the ones a previous run left.
fn secure_store_files(home: &FeldHome) -> Result<()> {
    let path = home.store_path();
    home.open_owner_file(&path, true)?;
    for suffix in ["-wal", "-shm"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        secure_if_exists(Path::new(&companion))?;
    }
    Ok(())
}

#[cfg(unix)]
fn secure_if_exists(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    absent_as_none(fs::set_permissions(path, fs::Permissions::from_mode(0o600)))
        .map(|_| ())
        .with_context(|| format!("secure {}", path.display()))
}

#[cfg(not(unix))]
fn secure_if_exists(_path: &Path) -> Result<()> {
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    absent_as_none(fs::remove_file(path))
        .map(|_| ())
        .with_context(|| format!("remove {}", path.display()))
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[tokio::test]
    async fn a_home_too_long_for_a_socket_serves_one_in_a_directory_of_its_own() {
        let directory = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(directory.path().join("h".repeat(120)))).unwrap();
        home.create().unwrap();
        let (_listener, path) = bind_socket(&home).unwrap();
        assert_eq!(path, home.short_socket_path());
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        remove_if_exists(&path).unwrap();
    }

    #[test]
    fn a_socket_directory_others_may_use_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let shared = directory.path().join("feld-shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(secure_socket_dir(&shared).is_err());
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o700)).unwrap();
        secure_socket_dir(&shared).unwrap();
    }

    #[test]
    fn the_store_and_its_wal_files_are_owner_only() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
        home.create().unwrap();

        secure_store_files(&home).unwrap();
        let store = Store::open(&home.store_path()).unwrap();

        for name in ["feld.db", "feld.db-wal", "feld.db-shm"] {
            assert_eq!(mode(&home.admin_dir().join(name)), 0o600, "{name}");
        }
        drop(store);
    }

    #[test]
    fn wal_files_left_open_to_others_become_owner_only() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
        home.create().unwrap();
        for name in ["feld.db-wal", "feld.db-shm"] {
            let path = home.admin_dir().join(name);
            fs::write(&path, b"").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }

        secure_store_files(&home).unwrap();

        for name in ["feld.db", "feld.db-wal", "feld.db-shm"] {
            assert_eq!(mode(&home.admin_dir().join(name)), 0o600, "{name}");
        }
    }

    /// A host where `[::1]` takes a listener, which the IPv6 tests need.
    fn has_ipv6() -> bool {
        std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok()
    }

    #[test]
    fn the_api_listens_on_both_loopback_addresses_on_one_port() {
        let listeners = bind_any_port().unwrap();
        let addresses: Vec<SocketAddr> = listeners
            .iter()
            .map(|listener| listener.local_addr().unwrap())
            .collect();

        let port = addresses[0].port();
        let mut expected = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)];
        if has_ipv6() {
            expected.push(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port));
        }
        assert_eq!(addresses, expected);
    }

    #[test]
    fn a_port_another_process_holds_on_ipv6_loopback_is_in_use() {
        if !has_ipv6() {
            return;
        }
        let holder = std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
        let port = holder.local_addr().unwrap().port();

        let error = bind_port(port).unwrap_err();

        assert!(error.is::<PortInUse>(), "{error:#}");
        assert!(!port_free(port).unwrap());
    }
}
