use std::collections::BTreeSet;

use axum::body::{Body, to_bytes};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;

use crate::console::redact_json;
use crate::service::credential;
use feld_core::model::{Grant, Requester};

use super::AppState;
use super::error::{ApiError, error, internal};

pub(super) struct SelfCaller {
    pub(super) session_id: String,
    pub(super) token: String,
}

#[derive(Clone, Debug)]
pub(super) struct AuthContext {
    pub(super) requester: Requester,
    pub(super) session_id: Option<String>,
    pub(super) token: Option<String>,
    /// The `id` of the browser token of the cookie, when it decided the caller.
    pub(super) browser: Option<String>,
    /// The grants of the Actor of the Session token, which [`super::access`] reads.
    pub(super) grants: BTreeSet<Grant>,
}

/// A caller of the Control API, judged before the request is read.
pub(super) struct Caller(pub(super) AuthContext);

impl FromRequestParts<AppState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        authorize(parts, state).map(Self)
    }
}

/// A caller of the event stream, judged before the request is read: the `id` of its
/// browser token or its Session token, whose end closes the stream, and the Workspace of
/// its Session token, the only one whose events it reads.
pub(super) struct StreamCaller {
    pub(super) browser: Option<String>,
    pub(super) session_token: Option<String>,
    pub(super) workspace: Option<String>,
}

impl FromRequestParts<AppState> for StreamCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        authorize(parts, state).map(|context| Self {
            browser: context.browser,
            session_token: context.token,
            workspace: match context.requester {
                Requester::Admin => None,
                Requester::Actor { workspace, .. } => Some(workspace),
            },
        })
    }
}

/// The caller of a Control API request that names the `spawner`, the `from` or the
/// requester, judged before the request is read.
pub(super) struct RequesterCaller(pub(super) AuthContext);

impl FromRequestParts<AppState> for RequesterCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        authorize_requester(parts, state).map(Self)
    }
}

/// The Collector with the internal token, judged before the request is read.
pub(super) struct InternalCaller;

impl FromRequestParts<AppState> for InternalCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        authorize_internal(&parts.headers, state).map(|()| Self)
    }
}

/// An Admin token, the local token or a browser token, judged before the request is read.
pub(super) struct AdminCaller;

impl FromRequestParts<AppState> for AdminCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        authorize_admin(parts, state).map(|()| Self)
    }
}

/// The local token alone, which manages pairing, judged before the request is read.
pub(super) struct LocalCaller;

impl FromRequestParts<AppState> for LocalCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        authorize_local(parts, state).map(|()| Self)
    }
}

/// A browser token in its cookie and key: the `id` of the token and whether its response cookie is
/// sent over HTTPS, judged before the request is read.
pub(super) struct BrowserCaller {
    pub(super) id: String,
    pub(super) secure_cookie: bool,
}

impl FromRequestParts<AppState> for BrowserCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        authorize_browser(parts, state).map(|id| Self {
            id,
            secure_cookie: secure_cookie(&parts.headers),
        })
    }
}

/// A pairing request, which carries no token: the credential of the browser token it still
/// holds, after its `Origin` is checked.
pub(super) struct PairCaller {
    pub(super) replacing: Option<String>,
    pub(super) secure_cookie: bool,
}

impl FromRequestParts<AppState> for PairCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        if !same_origin(&parts.headers) {
            return Err(cross_origin());
        }
        Ok(Self {
            replacing: browser_credential(&parts.headers, state),
            secure_cookie: secure_cookie(&parts.headers),
        })
    }
}

impl FromRequestParts<AppState> for SelfCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        authorize_self(parts, state)
    }
}

pub(super) async fn redact_response(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let preserve_attach_token =
        request.method() == Method::POST && request.uri().path().ends_with("/attach");
    let response = next.run(request).await;
    if response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|content_type| !content_type.starts_with("application/json"))
    {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(cause) => {
            return internal(
                anyhow::Error::new(cause).context("daemon response could not be encoded safely"),
            )
            .into_response();
        }
    };
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    let attach_token = preserve_attach_token
        .then(|| value.get("token").cloned())
        .flatten();
    redact_json(&mut value, &state.tokens.secrets());
    if let Some(token) = attach_token
        && let Some(object) = value.as_object_mut()
    {
        object.insert("token".to_owned(), token);
    }
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    match serde_json::to_vec(&value) {
        Ok(body) => Response::from_parts(parts, Body::from(body)),
        Err(cause) => internal(
            anyhow::Error::new(cause).context("daemon response could not be encoded safely"),
        )
        .into_response(),
    }
}

/// The header that carries the key of the browser token beside its cookie. The page keeps the
/// key in storage of its own origin, which, unlike the cookie, a server on another port of
/// the same host never receives.
const BROWSER_KEY: &str = "x-feld-browser-key";

fn supplied_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
        .map(|(_, token)| token)
}

/// The value of the cookie `name`.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// The credential of the browser token the cookie and [`BROWSER_KEY`] carry together.
fn browser_credential(headers: &HeaderMap, state: &AppState) -> Option<String> {
    let token = cookie(headers, state.browsers.cookie())?;
    let key = headers.get(BROWSER_KEY)?.to_str().ok()?;
    Some(credential(token, key))
}

/// The `id` of the valid browser token the cookie and its key carry, when the request has
/// no `Authorization`, which alone decides the caller when it is there. A request other
/// than `GET` must come from the origin it is sent to.
fn browser_token(parts: &Parts, state: &AppState) -> Result<Option<String>, ApiError> {
    if parts
        .headers
        .contains_key(axum::http::header::AUTHORIZATION)
    {
        return Ok(None);
    }
    let Some(credential) = browser_credential(&parts.headers, state) else {
        return Ok(None);
    };
    let Some(browser) = state.browsers.authenticate(&credential).map_err(internal)? else {
        return Ok(None);
    };
    if !matches!(parts.method, Method::GET | Method::HEAD) && !same_origin(&parts.headers) {
        return Err(cross_origin());
    }
    state.browsers.record_use(&browser).map_err(internal)?;
    Ok(Some(browser.id))
}

/// Whether `Origin` names the host and port the request is sent to: `X-Forwarded-Host`,
/// or `Host` without it. A port left out is the default of the scheme of `Origin`.
fn same_origin(headers: &HeaderMap) -> bool {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
    };
    let Some((scheme, origin)) = header("origin").and_then(|origin| origin.split_once("://"))
    else {
        return false;
    };
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        _ => return false,
    };
    let target = header("x-forwarded-host")
        .map(|hosts| hosts.split(',').next().unwrap_or_default().trim())
        .or_else(|| header("host"));
    match (
        target.and_then(|target| authority(target, default_port)),
        authority(origin, default_port),
    ) {
        (Some(target), Some(origin)) => target == origin,
        _ => false,
    }
}

/// Whether the browser reached the Web UI over HTTPS, as stated by the same-origin request.
/// A reverse proxy forwards the browser's public address in `Origin`; direct loopback HTTP
/// therefore leaves `Secure` off so WebKit will keep the cookie.
fn secure_cookie(headers: &HeaderMap) -> bool {
    headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .and_then(|origin| origin.trim().split_once("://"))
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("https"))
}

/// The lowercase host and the port of `host[:port]`, with `default_port` for a port left out.
pub(super) fn authority(value: &str, default_port: u16) -> Option<(String, u16)> {
    let (host, port) = match value.strip_prefix('[') {
        Some(rest) => {
            let (host, after) = rest.split_once(']')?;
            (format!("[{host}]"), after.strip_prefix(':'))
        }
        None => match value.rsplit_once(':') {
            Some((host, port)) => (host.to_owned(), Some(port)),
            None => (value.to_owned(), None),
        },
    };
    if host.is_empty() || host.contains('/') {
        return None;
    }
    let port = match port {
        Some(port) => port.parse().ok()?,
        None => default_port,
    };
    Some((host.to_ascii_lowercase(), port))
}

fn cross_origin() -> ApiError {
    error(
        StatusCode::FORBIDDEN,
        "the Origin of this request is not the address it is sent to",
    )
}

/// Whether a supplied token is the local token or the internal token, compared in constant
/// time.
pub(super) fn is_token(supplied: &str, expected: &str) -> bool {
    supplied.as_bytes().ct_eq(expected.as_bytes()).into()
}

fn unauthorized() -> ApiError {
    error(StatusCode::UNAUTHORIZED, "a valid Feld token is required")
}

fn authorize(parts: &Parts, state: &AppState) -> Result<AuthContext, ApiError> {
    let supplied = supplied_token(&parts.headers);
    if supplied.is_some_and(|token| is_token(token, &state.token)) {
        return Ok(AuthContext {
            requester: Requester::Admin,
            session_id: None,
            token: None,
            browser: None,
            grants: BTreeSet::new(),
        });
    }
    let authenticated = supplied
        .map(|token| state.tokens.authenticate(token))
        .transpose()
        .map_err(internal)?
        .flatten();
    if let Some(authenticated) = authenticated {
        return Ok(AuthContext {
            requester: authenticated.requester,
            session_id: Some(authenticated.session_id),
            token: supplied.map(str::to_owned),
            browser: None,
            grants: authenticated.grants,
        });
    }
    if let Some(browser) = browser_token(parts, state)? {
        return Ok(AuthContext {
            requester: Requester::Admin,
            session_id: None,
            token: None,
            browser: Some(browser),
            grants: BTreeSet::new(),
        });
    }
    Err(unauthorized())
}

/// The caller of a Control API request that names the `spawner`, the `from` or the
/// requester. The token of a closed Session is refused as session closed.
fn authorize_requester(parts: &Parts, state: &AppState) -> Result<AuthContext, ApiError> {
    authorize(parts, state).map_err(|refused| match supplied_token(&parts.headers) {
        Some(token) if refused.status == StatusCode::UNAUTHORIZED => refused_token(state, token),
        _ => refused,
    })
}

/// The caller of the Runtime API: the open Session its Session token names. An Admin token
/// names no Session, and the token of a closed Session is refused as session closed.
fn authorize_self(parts: &Parts, state: &AppState) -> Result<SelfCaller, ApiError> {
    let Some(token) = supplied_token(&parts.headers) else {
        return Err(match browser_token(parts, state)? {
            Some(_) => self_forbidden(),
            None => unauthorized(),
        });
    };
    if is_token(token, &state.token) {
        return Err(self_forbidden());
    }
    match state.tokens.authenticate(token).map_err(internal)? {
        Some(authenticated) => Ok(SelfCaller {
            session_id: authenticated.session_id,
            token: token.to_owned(),
        }),
        None => Err(refused_token(state, token)),
    }
}

fn self_forbidden() -> ApiError {
    error(
        StatusCode::FORBIDDEN,
        "the Runtime API requires a Session token",
    )
}

/// Why the Runtime API refuses a token that names no open Session: its Session closed,
/// or it names none at all.
pub(super) fn refused_token(state: &AppState, token: &str) -> ApiError {
    match state.tokens.closed_session(token) {
        Ok(Some(session)) => error(
            StatusCode::CONFLICT,
            &format!("session closed: Session '{session}' is already closed"),
        ),
        Ok(None) => unauthorized(),
        Err(error_value) => internal(error_value),
    }
}

/// The caller of an operation of the Admin alone: shutdown, restart, and attach. A Session
/// token is refused as the wrong kind of token.
fn authorize_admin(parts: &Parts, state: &AppState) -> Result<(), ApiError> {
    match supplied_token(&parts.headers) {
        Some(token) if is_token(token, &state.token) => Ok(()),
        Some(token) if state.tokens.issued(token).map_err(internal)? => Err(error(
            StatusCode::FORBIDDEN,
            "this operation requires the local token or a browser token",
        )),
        _ if browser_token(parts, state)?.is_some() => Ok(()),
        _ => Err(unauthorized()),
    }
}

/// The caller of the management of pairing, which only the local token calls. A Session
/// token and a browser token are refused as the wrong kind of token.
fn authorize_local(parts: &Parts, state: &AppState) -> Result<(), ApiError> {
    let wrong_kind = || {
        error(
            StatusCode::FORBIDDEN,
            "this operation requires the local token",
        )
    };
    match supplied_token(&parts.headers) {
        Some(token) if is_token(token, &state.token) => Ok(()),
        Some(token) if state.tokens.issued(token).map_err(internal)? => Err(wrong_kind()),
        _ if browser_token(parts, state)?.is_some() => Err(wrong_kind()),
        _ => Err(unauthorized()),
    }
}

/// The caller of unpairing: the browser token of the cookie. The local token and a Session
/// token are refused as the wrong kind of token.
fn authorize_browser(parts: &Parts, state: &AppState) -> Result<String, ApiError> {
    match supplied_token(&parts.headers) {
        Some(token)
            if is_token(token, &state.token) || state.tokens.issued(token).map_err(internal)? =>
        {
            Err(error(
                StatusCode::FORBIDDEN,
                "this operation requires a browser token",
            ))
        }
        _ => browser_token(parts, state)?.ok_or_else(unauthorized),
    }
}

/// The caller of the telemetry intake: the Collector with the internal token.
fn authorize_internal(headers: &HeaderMap, state: &AppState) -> Result<(), ApiError> {
    if supplied_token(headers).is_some_and(|token| is_token(token, &state.internal_token)) {
        Ok(())
    } else {
        Err(unauthorized())
    }
}

#[cfg(test)]
mod tests {
    use super::is_token;

    #[test]
    fn is_token_accepts_only_the_same_token() {
        assert!(is_token("feld-token", "feld-token"));
        assert!(!is_token("feld-tokem", "feld-token"));
        assert!(!is_token("feld-toke", "feld-token"));
        assert!(!is_token("", "feld-token"));
    }
}
