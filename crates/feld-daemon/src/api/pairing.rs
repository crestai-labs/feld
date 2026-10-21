use axum::Json;
use axum::extract::{FromRequest, Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use feld_core::model::PairingCode;

use super::AppState;
use super::auth::{BrowserCaller, LocalCaller, PairCaller};
use super::error::{ApiError, error, internal};
use super::query::ApiJson;
use super::types::{BrowserList, BrowserResponse, PairResponse};

/// The browser token cookie's attributes; `Secure` is added for HTTPS requests, and its
/// lifetime is the longest a token can live.
const COOKIE_ATTRIBUTES: &str = "HttpOnly; SameSite=Strict";
const COOKIE_MAX_AGE: i64 = crate::store::BROWSER_LIFETIME.num_seconds();

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PairingCodeRequest {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PairRequest {
    code: String,
}

pub(super) async fn create_pairing_code(
    State(state): State<AppState>,
    _caller: LocalCaller,
    request: Request,
) -> Result<Json<PairingCode>, ApiError> {
    // The body is optional: without one the token is named `browser`.
    let empty = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .is_some_and(|length| length == "0")
        || (!request.headers().contains_key(header::CONTENT_LENGTH)
            && !request.headers().contains_key(header::TRANSFER_ENCODING));
    let request = if empty {
        PairingCodeRequest::default()
    } else {
        ApiJson::<PairingCodeRequest>::from_request(request, &state)
            .await?
            .0
    };
    let name = request.name.unwrap_or_else(|| "browser".to_owned());
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "a browser token name must be non-empty and free of control characters",
        ));
    }
    Ok(Json(state.browsers.issue_code(&name)))
}

pub(super) async fn pair_browser(
    State(state): State<AppState>,
    caller: PairCaller,
    ApiJson(request): ApiJson<PairRequest>,
) -> Result<Response, ApiError> {
    let paired = state
        .browsers
        .pair(&request.code, &caller.replacing)
        .map_err(internal)?
        .ok_or_else(|| {
            error(
                StatusCode::UNAUTHORIZED,
                "the pairing code is unknown, expired, or already used",
            )
        })?;
    let cookie = browser_cookie(
        state.browsers.cookie(),
        &paired.token,
        caller.secure_cookie,
        COOKIE_MAX_AGE,
    );
    Ok(with_cookie(
        Json(PairResponse {
            browser: paired.browser,
            key: paired.key,
        })
        .into_response(),
        &cookie,
    ))
}

pub(super) async fn unpair_browser(
    State(state): State<AppState>,
    BrowserCaller { id, secure_cookie }: BrowserCaller,
) -> Result<Response, ApiError> {
    state.browsers.revoke(&id).map_err(internal)?;
    let cookie = browser_cookie(state.browsers.cookie(), "", secure_cookie, 0);
    Ok(with_cookie(StatusCode::NO_CONTENT.into_response(), &cookie))
}

pub(super) async fn list_browsers(
    State(state): State<AppState>,
    _caller: LocalCaller,
) -> Result<Json<BrowserList>, ApiError> {
    Ok(Json(BrowserList {
        browsers: state.browsers.list().map_err(internal)?,
    }))
}

pub(super) async fn revoke_browser(
    State(state): State<AppState>,
    _caller: LocalCaller,
    Path(browser): Path<String>,
) -> Result<Json<BrowserResponse>, ApiError> {
    state
        .browsers
        .revoke(&browser)
        .map_err(internal)?
        .map(|browser| Json(BrowserResponse { browser }))
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                &format!("browser token '{browser}' is not valid"),
            )
        })
}

fn with_cookie(mut response: Response, cookie: &str) -> Response {
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(cookie).expect("a browser token cookie is a valid header"),
    );
    response
}

fn browser_cookie(name: &str, value: &str, secure: bool, max_age: i64) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{name}={value}; {COOKIE_ATTRIBUTES}{secure}; Path=/; Max-Age={max_age}")
}
