//! The daemon's side of the spool (`docs/spec/design/spool.md`): it takes the requests a CLI
//! inside a Runtime's sandbox writes as files, hands them to the same router as TCP, and
//! writes the responses back as files.

use std::collections::HashMap;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use fs2::FileExt;
use notify::Watcher as _;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, watch};
use tokio::task::JoinSet;
use tower::ServiceExt as _;

use feld_core::model::Ownership;
use feld_core::session_token;
use feld_core::spool::{self as wire, Credential, Directory, Source, State};

use super::auth::is_token;
use super::{AppState, Route};

/// How often the daemon scans the spool besides the notifications, checks whether the CLI
/// of a request it handles still waits, and removes what is left.
const SCAN_INTERVAL: Duration = Duration::from_secs(1);
/// How old a file nobody takes any more grows before the scan removes it.
const STALE_AGE: Duration = Duration::from_secs(10 * 60);
/// The largest request body the API reads (axum's default limit), which the file adds the
/// rest of the request to.
const BODY_LIMIT: u64 = 2 * 1024 * 1024;
const FILE_LIMIT: u64 = BODY_LIMIT + 64 * 1024;
/// How many requests of one directory the daemon handles at once. The rest wait in the
/// directory, so a sandbox that writes many requests delays only its own.
const HANDLING_LIMIT: usize = 16;
/// How many requests the daemon checks off the async workers at once. A directory checks one
/// at a time, so a slow check delays only its own directory.
const ADMIT_LIMIT: usize = 8;

/// Serves the spool of the Feld home until `shutdown`. The daemon creates the spool when it
/// starts, and the requests a daemon before it left are settled first.
pub async fn serve(state: AppState, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    let spool = state.home.spool_dir();
    let router = super::router(state.clone());
    arrange(&state);
    for (_, directory) in directories(&spool) {
        settle_left_behind(&directory);
    }

    // However many notifications come between two passes, one pass answers them all. The end
    // of a request wakes the pass too, for the requests its directory kept waiting.
    let woken = Arc::new(Notify::new());
    // Without notifications the scan still takes every request, only later.
    let watcher = notify::recommended_watcher({
        let woken = Arc::clone(&woken);
        move |event| {
            if wakes(&event) {
                woken.notify_one();
            }
        }
    })
    .and_then(|mut watcher| {
        watcher.watch(&spool, notify::RecursiveMode::Recursive)?;
        Ok(watcher)
    });
    if let Err(error) = &watcher {
        eprintln!(
            "Feld spool: notifications are unavailable, so requests wait for the scan: {error}"
        );
    }

    let mut handling = JoinSet::new();
    let mut limits: HashMap<PathBuf, (Arc<Semaphore>, Admission)> = HashMap::new();
    let admitting = Arc::new(Semaphore::new(ADMIT_LIMIT));
    let mut scan = tokio::time::interval(SCAN_INTERVAL);
    loop {
        tokio::select! {
            _ = scan.tick() => {
                arrange(&state);
                remove_stale(&spool, true);
                for (_, directory) in directories(&spool) {
                    remove_stale(&directory, false);
                }
            }
            () = woken.notified() => {}
            _ = shutdown.changed() => {}
        }
        if *shutdown.borrow() {
            break;
        }
        while handling.try_join_next().is_some() {}
        let present = directories(&spool);
        limits.retain(|path, _| present.iter().any(|(_, directory)| directory == path));
        for (owner, directory) in present {
            let (limit, admission) = limits.entry(directory.clone()).or_insert_with(|| {
                (
                    Arc::new(Semaphore::new(HANDLING_LIMIT)),
                    Admission {
                        directory: Arc::new(Semaphore::new(1)),
                        daemon: Arc::clone(&admitting),
                    },
                )
            });
            for (id, taken, permit) in take_waiting(&directory, limit) {
                let handled = handle(
                    state.clone(),
                    router.clone(),
                    admission.clone(),
                    Place {
                        owner: owner.clone(),
                        path: directory.clone(),
                    },
                    id,
                    taken,
                    shutdown.clone(),
                );
                let woken = Arc::clone(&woken);
                handling.spawn(async move {
                    handled.await;
                    drop(permit);
                    woken.notify_one();
                });
            }
        }
    }
    // A request this daemon was handling stays taken; the next daemon answers it.
    handling.abort_all();
    drop(watcher);
    Ok(())
}

/// Whether a notification may mean a request came. A pass reads the spool, and on Linux
/// reading a directory is itself a notification, so one that only reports a read would
/// wake the next pass without end.
fn wakes(event: &notify::Result<notify::Event>) -> bool {
    !matches!(event, Ok(event) if event.kind.is_access())
}

/// The permits a request holds while its caller is checked: one of its directory's and one
/// of the daemon's.
#[derive(Clone)]
struct Admission {
    directory: Arc<Semaphore>,
    daemon: Arc<Semaphore>,
}

impl Admission {
    /// Waits for both permits, and gives up with `None` once the CLI of `file` ends.
    async fn enter(&self, file: &File) -> Option<[OwnedSemaphorePermit; 2]> {
        // The semaphores are never closed.
        let acquire = async {
            let directory = Arc::clone(&self.directory).acquire_owned().await.ok()?;
            let daemon = Arc::clone(&self.daemon).acquire_owned().await.ok()?;
            Some([directory, daemon])
        };
        tokio::pin!(acquire);
        let mut check = tokio::time::interval(SCAN_INTERVAL);
        check.tick().await;
        loop {
            tokio::select! {
                permits = &mut acquire => return permits,
                _ = check.tick() => {
                    if abandoned(file) {
                        return None;
                    }
                }
            }
        }
    }
}

/// One directory of the spool: whose requests it takes, and where it is.
struct Place {
    owner: Directory,
    path: PathBuf,
}

/// Keeps the directories of the spool in step with the Sessions: the shared one, and one for
/// each open `owned` Session, which the start of the Session creates before its launch. The
/// directory of a Session that ended goes, with what is left in it, once no request in it
/// waits for its response.
fn arrange(state: &AppState) {
    let home = &state.home;
    if let Err(error) = home.create_owner_dir(&home.shared_spool_dir()) {
        eprintln!("Feld spool: the shared directory was not created: {error:#}");
    }
    let open = match state.sessions.current() {
        Ok(sessions) => sessions
            .into_iter()
            .filter(|session| session.ownership == Ownership::Owned)
            .map(|session| session.id)
            .collect::<std::collections::HashSet<_>>(),
        Err(error) => {
            eprintln!("Feld spool: the open Sessions were not read: {error:#}");
            return;
        }
    };
    for session in &open {
        let directory = home.session_spool_dir(session);
        if !directory.exists()
            && let Err(error) = home.create_owner_dir(&directory)
        {
            eprintln!(
                "Feld spool: {} was not created: {error:#}",
                directory.display()
            );
        }
    }
    for (owner, directory) in directories(&home.spool_dir()) {
        if let Directory::Session(session) = owner
            && !open.contains(&session)
            && !answering(&directory)
            && let Err(error) = fs::remove_dir_all(&directory)
            && error.kind() != ErrorKind::NotFound
        {
            eprintln!(
                "Feld spool: {} was not removed: {error}",
                directory.display()
            );
        }
    }
}

/// The directories of the spool the daemon takes requests from: a directory of the daemon's
/// own user, named as the shared one or a Session's.
fn directories(spool: &Path) -> Vec<(Directory, PathBuf)> {
    entries(spool)
        .into_iter()
        .filter_map(|name| {
            let owner = Directory::of_name(&name)?;
            let path = spool.join(&name);
            let metadata = fs::symlink_metadata(&path).ok()?;
            // SAFETY: geteuid has no preconditions and cannot fail.
            (metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() })
                .then_some((owner, path))
        })
        .collect()
}

/// Whether a request of `directory` waits for its response: one waiting, one being handled,
/// or a response its CLI has not read. A Session that stops itself through the spool ends
/// before its request is answered.
fn answering(directory: &Path) -> bool {
    entries(directory).iter().any(|name| {
        matches!(
            wire::parse_name(name),
            Some((_, State::Request | State::Taken | State::Response))
        )
    })
}

/// Takes the requests waiting in `directory` while `limit` has room, each with the permit
/// it holds until it is handled. A directory at its limit is not read.
fn take_waiting(
    directory: &Path,
    limit: &Arc<Semaphore>,
) -> Vec<(String, File, OwnedSemaphorePermit)> {
    if limit.available_permits() == 0 {
        return Vec::new();
    }
    let mut taken = Vec::new();
    for id in waiting_requests(directory) {
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            break;
        };
        if let Some(file) = take(directory, &id) {
            taken.push((id, file, permit));
        }
    }
    taken
}

/// The ids of the requests waiting in the spool.
fn waiting_requests(spool: &Path) -> Vec<String> {
    entries(spool)
        .into_iter()
        .filter_map(|name| match wire::parse_name(&name) {
            Some((id, State::Request)) => Some(id.to_owned()),
            _ => None,
        })
        .collect()
}

fn entries(spool: &Path) -> Vec<String> {
    let Ok(directory) = fs::read_dir(spool) else {
        return Vec::new();
    };
    directory
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect()
}

/// A file the daemon reads: a regular file of the daemon's own user.
fn trusted(metadata: &Metadata) -> bool {
    metadata.file_type().is_file() && metadata.uid() == unsafe { libc::geteuid() }
}

/// Takes request `id` by renaming it to taken, and opens it. `None` when the CLI withdrew
/// it first, or when the file is not one the daemon reads, which the scan removes later.
fn take(spool: &Path, id: &str) -> Option<File> {
    let request = spool.join(wire::file_name(id, State::Request));
    let taken = spool.join(wire::file_name(id, State::Taken));
    if !fs::symlink_metadata(&request).is_ok_and(|metadata| trusted(&metadata)) {
        return None;
    }
    fs::rename(&request, &taken).ok()?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&taken);
    match file {
        Ok(file) if file.metadata().is_ok_and(|metadata| trusted(&metadata)) => Some(file),
        _ => {
            remove(&taken);
            None
        }
    }
}

/// Whether the CLI that wrote a request has ended: its lock on the file is gone.
fn abandoned(file: &File) -> bool {
    file.try_lock_exclusive().is_ok()
}

/// Judges and handles one taken request, and writes its response unless its CLI ended or the
/// daemon is stopping.
async fn handle(
    state: AppState,
    router: Router,
    admission: Admission,
    place: Place,
    id: String,
    file: File,
    shutdown: watch::Receiver<bool>,
) {
    let Some(response) = answer(state, router, admission, &place, &id, &file).await else {
        return;
    };
    // A response that comes while the daemon stops may answer for the stop itself; the request
    // stays taken, and the next daemon answers it.
    if *shutdown.borrow() {
        return;
    }
    respond(&place.path, &id, &response);
}

/// The response to one taken request, or `None` when its CLI ended, which removes the request.
async fn answer(
    state: AppState,
    router: Router,
    admission: Admission,
    place: &Place,
    id: &str,
    file: &File,
) -> Option<wire::Response> {
    let taken = place.path.join(wire::file_name(id, State::Taken));
    if abandoned(file) {
        remove(&taken);
        return None;
    }
    // The check of the caller reads the process table, so it runs off the async workers.
    let Some(permits) = admission.enter(file).await else {
        remove(&taken);
        return None;
    };
    let admitted = {
        let id = id.to_owned();
        let file = file.try_clone();
        let owner = place.owner.clone();
        tokio::task::spawn_blocking(move || admit(&state, &owner, file, &id)).await
    };
    drop(permits);
    let request = match admitted {
        Ok(Ok(request)) => request,
        Ok(Err(refused)) => return Some(refused),
        Err(cause) => {
            return Some(failure(&format!(
                "the daemon failed while checking the request: {cause}"
            )));
        }
    };
    let mut answer = tokio::spawn(dispatch(router, request));
    let mut check = tokio::time::interval(SCAN_INTERVAL);
    check.tick().await;
    loop {
        tokio::select! {
            joined = &mut answer => break Some(joined.unwrap_or_else(|cause| {
                failure(&format!("the daemon failed while handling the request: {cause}"))
            })),
            _ = check.tick() => {
                // The CLI ended: the request is cancelled as a TCP request whose connection
                // closed.
                if abandoned(file) {
                    answer.abort();
                    remove(&taken);
                    return None;
                }
            }
        }
    }
}

/// The HTTP request a taken file of the directory of `owner` carries, once its shape, path
/// and token pass, or the response that refuses it.
fn admit(
    state: &AppState,
    owner: &Directory,
    file: std::io::Result<File>,
    id: &str,
) -> Result<axum::http::Request<Body>, wire::Response> {
    let file = file.map_err(|_| unreadable(id))?;
    let bytes = read_limited(file, id)?;
    let request: wire::Request = serde_json::from_slice(&bytes).map_err(|error| {
        refusal(
            StatusCode::BAD_REQUEST,
            &format!("the spool request is not valid JSON: {error}"),
        )
    })?;
    if request.version != wire::VERSION {
        return Err(refusal(
            StatusCode::BAD_REQUEST,
            &format!("the spool request version {} is unknown", request.version),
        ));
    }
    let uri = origin_form(&request.path).ok_or_else(|| {
        refusal(
            StatusCode::BAD_REQUEST,
            "the spool request path is not an absolute path",
        )
    })?;
    let path = uri.path();
    if path == "/v1/events" || path == "/" || path == "/ui" || path.starts_with("/ui/") {
        return Err(refusal(
            StatusCode::BAD_REQUEST,
            "this API path is not available over the spool",
        ));
    }
    for credential in &request.credentials {
        if !session_token_kind(state, &credential.token) {
            return Err(refusal(
                StatusCode::FORBIDDEN,
                "the spool accepts Session tokens only",
            ));
        }
    }
    let caller = host_pid(&request.caller);
    let verified: Vec<&Credential> = request
        .credentials
        .iter()
        .filter(|credential| verified(state, credential, caller))
        .collect();
    if verified.is_empty() {
        return Err(refusal(
            StatusCode::UNAUTHORIZED,
            "No Session token: run this inside an attached or owned Feld Session",
        ));
    }
    let token = verified
        .into_iter()
        .find(|credential| belongs(state, credential, owner))
        .map(|credential| credential.token.clone())
        .ok_or_else(|| {
            refusal(
                StatusCode::FORBIDDEN,
                "the Session token does not belong to this directory of the spool",
            )
        })?;
    build(request, uri, &token)
        .map_err(|error| refusal(StatusCode::BAD_REQUEST, &format!("{error:#}")))
}

/// The bytes of a request file up to `FILE_LIMIT`, or the response that refuses a larger
/// one. The writer holds the file open, so the limit holds while it reads, not before.
fn read_limited(file: impl Read, id: &str) -> Result<Vec<u8>, wire::Response> {
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable(id))?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(refusal(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the spool request is larger than the daemon reads",
        ));
    }
    Ok(bytes)
}

/// Whether `token` may ride the spool: it is none of the local token, the internal token or
/// a browser token.
fn session_token_kind(state: &AppState, token: &str) -> bool {
    !is_token(token, &state.token)
        && !is_token(token, &state.internal_token)
        && !state
            .browsers
            .authenticate(token)
            .is_ok_and(|browser| browser.is_some())
}

/// Whether a Session token the CLI found belongs to the caller, checked the way the CLI
/// checks it over TCP (`auth.md`). The environment of an `owned` Session needs no check. A
/// token of the self index passes only for an `attached` Session, against the self index
/// entry that holds it, and only for a caller the daemon finds.
fn verified(state: &AppState, credential: &Credential, caller: Option<u32>) -> bool {
    if credential.source == Source::Environment {
        return true;
    }
    let Some(caller) = caller else {
        return false;
    };
    let Ok(Some(authenticated)) = state.tokens.authenticate(&credential.token) else {
        return false;
    };
    let attached = state
        .sessions
        .get(&authenticated.session_id)
        .ok()
        .flatten()
        .is_some_and(|session| session.ownership == Ownership::Attached);
    if !attached {
        return false;
    }
    let Ok(Some(index)) =
        session_token::index_of(&state.home, &authenticated.session_id, &credential.token)
    else {
        return false;
    };
    session_token::admits(&index, credential, caller, state.registry.as_ref()).unwrap_or_else(
        |error| {
            eprintln!("Feld spool: the caller {caller} was not checked: {error:#}");
            false
        },
    )
}

/// Whether a request with this credential may come through the directory of `owner`: the
/// directory of an `owned` Session takes that Session's token only, and the shared one the
/// tokens of the `attached` Sessions, which [`verified`] checks against the self index.
fn belongs(state: &AppState, credential: &Credential, owner: &Directory) -> bool {
    match owner {
        Directory::Shared => credential.source != Source::Environment,
        Directory::Session(session) => state
            .tokens
            .authenticate(&credential.token)
            .ok()
            .flatten()
            .is_some_and(|authenticated| &authenticated.session_id == session),
    }
}

/// The pid the caller of a request has in the daemon's PID namespace. A caller inside a
/// sandbox with its own PID namespace (Linux's bubblewrap) states the pid it has there, and
/// the daemon finds the process of that namespace whose innermost pid it is.
fn host_pid(caller: &wire::Caller) -> Option<u32> {
    let own = fs::read_link("/proc/self/ns/pid")
        .ok()
        .map(|link| link.display().to_string());
    let Some(namespace) = caller.pid_namespace.as_deref() else {
        return Some(caller.pid);
    };
    if own.as_deref() == Some(namespace) {
        return Some(caller.pid);
    }
    let processes = fs::read_dir("/proc").ok()?;
    processes
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .find(|pid| {
            fs::read_link(format!("/proc/{pid}/ns/pid"))
                .is_ok_and(|link| link.display().to_string() == namespace)
                && fs::read_to_string(format!("/proc/{pid}/status"))
                    .ok()
                    .as_deref()
                    .and_then(innermost_pid)
                    == Some(caller.pid)
        })
}

/// The last value of the `NSpid` line of `/proc/<pid>/status`: the pid the process has in
/// its innermost PID namespace.
fn innermost_pid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("NSpid:"))?
        .split_whitespace()
        .last()?
        .parse()
        .ok()
}

/// `path` as a URI when it is an origin-form path with its query, which the router reads as
/// is, and none of the forms that carry a scheme or an authority.
fn origin_form(path: &str) -> Option<Uri> {
    path.parse::<Uri>().ok().filter(|uri| {
        uri.scheme().is_none() && uri.authority().is_none() && uri.path().starts_with('/')
    })
}

fn build(request: wire::Request, uri: Uri, token: &str) -> Result<axum::http::Request<Body>> {
    let mut builder = axum::http::Request::builder()
        .method(Method::from_bytes(request.method.as_bytes()).context("read the method")?)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .extension(Route::Spool);
    if let Some(content_type) = &request.content_type {
        builder = builder.header(
            header::CONTENT_TYPE,
            HeaderValue::from_str(content_type).context("read the content type")?,
        );
    }
    builder
        .body(request.body.map(Body::from).unwrap_or_else(Body::empty))
        .context("read the request")
}

async fn dispatch(router: Router, request: axum::http::Request<Body>) -> wire::Response {
    let Ok(response) = router.oneshot(request).await;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    match to_bytes(response.into_body(), usize::MAX).await {
        Ok(body) => wire::Response {
            version: wire::VERSION,
            status,
            content_type,
            body: (!body.is_empty()).then(|| String::from_utf8_lossy(&body).into_owned()),
        },
        Err(cause) => failure(&format!("the daemon response could not be read: {cause}")),
    }
}

/// A failure of the daemon itself, which the daemon log keeps too.
fn failure(message: &str) -> wire::Response {
    eprintln!("Feld spool: {message}");
    refusal(StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn refusal(status: StatusCode, message: &str) -> wire::Response {
    wire::Response {
        version: wire::VERSION,
        status: status.as_u16(),
        content_type: Some("application/json".to_owned()),
        body: Some(serde_json::json!({ "message": message }).to_string()),
    }
}

fn unreadable(id: &str) -> wire::Response {
    refusal(
        StatusCode::BAD_REQUEST,
        &format!("the spool request {id} could not be read"),
    )
}

/// Writes the response of request `id` into place, then lets the taken request go. A partial
/// response a daemon before this one left is replaced. A response that could not be written
/// leaves the request taken, which the next daemon answers.
fn respond(spool: &Path, id: &str, response: &wire::Response) {
    let partial = spool.join(wire::partial_name(id, State::Response));
    remove(&partial);
    let written = serde_json::to_vec(response)
        .map_err(std::io::Error::other)
        .and_then(|bytes| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&partial)?;
            file.write_all(&bytes)
        })
        .and_then(|()| fs::rename(&partial, spool.join(wire::file_name(id, State::Response))));
    if let Err(error) = written {
        eprintln!("Feld spool: the response to {id} was not written: {error}");
        remove(&partial);
        return;
    }
    remove(&spool.join(wire::file_name(id, State::Taken)));
}

/// Settles what a daemon before this one left: a request it was handling may have been
/// applied, which its CLI learns from the response. A waiting request is taken as usual.
fn settle_left_behind(spool: &Path) {
    for name in entries(spool) {
        if let Some((id, State::Taken)) = wire::parse_name(&name) {
            respond(
                spool,
                id,
                &refusal(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "the Feld daemon restarted while handling the request; it may have been applied",
                ),
            );
        }
    }
}

/// Removes the files nobody takes any more once they are old: responses, withdrawn
/// requests, partial files, and the files the daemon does not read. At the top of the spool,
/// where only the directories of the spool live, every other file is one of these.
fn remove_stale(directory: &Path, top: bool) {
    let now = SystemTime::now();
    let kept: Vec<PathBuf> = if top {
        directories(directory)
            .into_iter()
            .map(|(_, path)| path)
            .collect()
    } else {
        Vec::new()
    };
    for name in entries(directory) {
        let path = directory.join(&name);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        let live = kept.contains(&path)
            || (!top
                && match wire::parse_name(&name) {
                    Some((_, State::Taken)) => true,
                    Some((_, State::Request)) => trusted(&metadata),
                    _ => false,
                });
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > STALE_AGE);
        if !live && stale {
            if metadata.is_dir() {
                let _ = fs::remove_dir_all(&path);
            } else {
                remove(&path);
            }
        }
    }
}

fn remove(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != ErrorKind::NotFound
    {
        eprintln!("Feld spool: {} was not removed: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_origin_form_path_is_a_spool_path() {
        let uri = origin_form("/v1/self?wait").unwrap();
        assert_eq!(uri.path(), "/v1/self");
        assert_eq!(uri.query(), Some("wait"));
        for path in [
            "http://x/v1/events",
            "HTTP://x/v1/self",
            "x:80",
            "*",
            "v1/events",
            "",
        ] {
            assert_eq!(origin_form(path), None, "{path}");
        }
    }

    #[test]
    fn the_innermost_pid_is_the_last_of_nspid() {
        let status = "Name:\tfeld\nTgid:\t4021\nPid:\t4021\nNSpid:\t4021\t12\nPPid:\t4000\n";
        assert_eq!(innermost_pid(status), Some(12));
        assert_eq!(innermost_pid("Name:\tfeld\nNSpid:\t77\n"), Some(77));
        assert_eq!(innermost_pid("Name:\tfeld\n"), None);
    }

    #[test]
    fn a_caller_without_a_namespace_or_in_the_daemons_is_taken_at_its_word() {
        let caller = wire::Caller::current();
        assert_eq!(host_pid(&caller), Some(std::process::id()));
        let plain = wire::Caller {
            pid: 4021,
            pid_namespace: None,
        };
        assert_eq!(host_pid(&plain), Some(4021));
    }

    #[test]
    fn a_request_file_that_grows_past_the_limit_while_open_is_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request");
        fs::write(&path, b"{}").unwrap();
        let file = File::open(&path).unwrap();
        assert_eq!(read_limited(&file, "id").unwrap(), b"{}");

        let file = File::open(&path).unwrap();
        let mut writer = OpenOptions::new().append(true).open(&path).unwrap();
        writer.write_all(&vec![b' '; FILE_LIMIT as usize]).unwrap();
        let refused = read_limited(&file, "id").unwrap_err();
        assert_eq!(refused.status, StatusCode::PAYLOAD_TOO_LARGE.as_u16());
    }

    #[test]
    fn a_directory_answers_until_its_responses_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let id = "0".repeat(32);
        fs::write(
            dir.path().join(wire::partial_name(&id, State::Request)),
            b"",
        )
        .unwrap();
        fs::write(dir.path().join(wire::file_name(&id, State::Withdrawn)), b"").unwrap();
        assert!(!answering(dir.path()));
        for state in [State::Request, State::Taken, State::Response] {
            let file = dir.path().join(wire::file_name(&id, state));
            fs::write(&file, b"").unwrap();
            assert!(answering(dir.path()), "{}", file.display());
            fs::remove_file(&file).unwrap();
        }
    }

    #[test]
    fn a_notification_of_a_read_does_not_wake_the_scan() {
        use notify::event::{
            AccessKind, AccessMode, CreateKind, EventKind, ModifyKind, RenameMode,
        };
        let event = |kind| Ok(notify::Event::new(kind));
        assert!(!wakes(&event(EventKind::Access(AccessKind::Open(
            AccessMode::Any
        )))));
        assert!(!wakes(&event(EventKind::Access(AccessKind::Close(
            AccessMode::Read
        )))));
        assert!(wakes(&event(EventKind::Create(CreateKind::File))));
        assert!(wakes(&event(EventKind::Modify(ModifyKind::Name(
            RenameMode::To
        )))));
        assert!(wakes(&Err(notify::Error::generic("lost"))));
    }

    /// inotify reports the opening of a watched directory, so the pass that reads the spool
    /// is notified of its own reads; none of those notifications wakes the next pass.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_pass_over_the_spool_does_not_wake_the_next() {
        use std::sync::mpsc::channel;

        let dir = tempfile::tempdir().unwrap();
        let spool = dir.path();
        fs::create_dir(spool.join(feld_core::spool::SHARED_DIR)).unwrap();
        let (sender, notifications) = channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
        watcher
            .watch(spool, notify::RecursiveMode::Recursive)
            .unwrap();
        let collect = || {
            let mut events = Vec::new();
            while let Ok(event) = notifications.recv_timeout(Duration::from_millis(300)) {
                events.push(event);
            }
            events
        };

        for (_, directory) in directories(spool) {
            assert!(waiting_requests(&directory).is_empty());
        }
        let read = collect();
        assert!(!read.is_empty(), "inotify reports the reads of the spool");
        assert!(read.iter().all(|event| !wakes(event)), "{read:?}");

        let id = "0".repeat(32);
        fs::write(
            spool
                .join(feld_core::spool::SHARED_DIR)
                .join(wire::file_name(&id, State::Request)),
            b"{}",
        )
        .unwrap();
        assert!(collect().iter().any(wakes));
    }

    #[test]
    fn a_directory_hands_out_no_more_requests_than_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..HANDLING_LIMIT + 4 {
            let id = format!("{n:032}");
            fs::write(dir.path().join(wire::file_name(&id, State::Request)), b"").unwrap();
        }
        let limit = Arc::new(Semaphore::new(HANDLING_LIMIT));
        let mut taken = take_waiting(dir.path(), &limit);
        assert_eq!(taken.len(), HANDLING_LIMIT);
        assert_eq!(waiting_requests(dir.path()).len(), 4);
        assert!(take_waiting(dir.path(), &limit).is_empty());

        taken.truncate(HANDLING_LIMIT - 1);
        assert_eq!(take_waiting(dir.path(), &limit).len(), 1);
        assert_eq!(waiting_requests(dir.path()).len(), 3);
    }

    /// A directory checks one request at a time, so a slow check of one directory leaves the
    /// daemon's permits to the others.
    #[tokio::test]
    async fn a_directory_checks_one_request_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request");
        let file = File::create(&path).unwrap();
        let lock = File::open(&path).unwrap();
        lock.lock_exclusive().unwrap();
        let daemon = Arc::new(Semaphore::new(ADMIT_LIMIT));
        let admission = || Admission {
            directory: Arc::new(Semaphore::new(1)),
            daemon: Arc::clone(&daemon),
        };
        let (slow, other) = (admission(), admission());

        let held = slow.enter(&file).await.unwrap();
        let waiting = tokio::time::timeout(Duration::from_millis(100), slow.enter(&file)).await;
        assert!(waiting.is_err(), "the directory waits for its own check");
        assert_eq!(daemon.available_permits(), ADMIT_LIMIT - 1);
        let checked = other.enter(&file).await;
        assert!(checked.is_some(), "another directory is checked meanwhile");
        drop(held);
    }

    /// A request waiting for its check is cancelled once its CLI ends.
    #[tokio::test]
    async fn a_request_waiting_for_its_check_is_cancelled_with_its_cli() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request");
        let file = File::create(&path).unwrap();
        let lock = File::open(&path).unwrap();
        lock.lock_exclusive().unwrap();
        let admission = Admission {
            directory: Arc::new(Semaphore::new(1)),
            daemon: Arc::new(Semaphore::new(ADMIT_LIMIT)),
        };
        let _held = admission.enter(&file).await.unwrap();

        let waiting = admission.enter(&file);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(SCAN_INTERVAL * 2, &mut waiting)
                .await
                .is_err()
        );
        drop(lock);
        assert!(waiting.await.is_none());
    }

    #[test]
    fn a_caller_of_a_namespace_nobody_is_in_is_not_found() {
        let stranger = wire::Caller {
            pid: 1,
            pid_namespace: Some("pid:[1]".to_owned()),
        };
        assert_eq!(host_pid(&stranger), None);
    }
}
