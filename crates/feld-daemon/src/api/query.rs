use axum::Json;
use axum::extract::{FromRequest, FromRequestParts, Query, Request};
use axum::http::StatusCode;
use axum::http::request::Parts;
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize};

use feld_core::time;

use super::error::{ApiError, error};

/// `Json` whose rejection answers as an `Error`.
pub(super) struct ApiJson<T>(pub(super) T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, ApiError> {
        Json::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|rejection| error(rejection.status(), &rejection.body_text()))
    }
}

/// `Query` whose rejection answers as an `Error`.
pub(super) struct ApiQuery<T>(pub(super) T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        Query::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|rejection| error(rejection.status(), &rejection.body_text()))
    }
}

/// A boolean query parameter, which is true when it comes without a value.
fn flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    match String::deserialize(deserializer)?.as_str() {
        "" | "true" => Ok(true),
        "false" => Ok(false),
        other => Err(D::Error::custom(format!(
            "expected `true`, `false` or no value, found `{other}`"
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct SessionMetricsQuery {
    pub(super) start: Option<String>,
    pub(super) end: Option<String>,
    pub(super) bucket: Option<u64>,
    pub(super) metric: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct CursorQuery {
    pub(super) after: Option<u64>,
    pub(super) before: Option<u64>,
    pub(super) limit: Option<usize>,
    #[serde(default, deserialize_with = "flag")]
    pub(super) tail: bool,
}

/// The Console's cursor over transcript entries. The cursor is the Runtime Adapter's own
/// string, which core passes back to it as it came.
#[derive(Debug, Deserialize, Serialize)]
pub(super) struct TranscriptQuery {
    pub(super) after: Option<String>,
    pub(super) before: Option<String>,
    pub(super) limit: Option<usize>,
    #[serde(default, deserialize_with = "flag")]
    pub(super) tail: bool,
    #[serde(default)]
    pub(super) view: TranscriptView,
}

/// Which records a transcript read returns: the Conversation view leaves out `other`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum TranscriptView {
    Conversation,
    #[default]
    All,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceQuery {
    #[serde(default, deserialize_with = "flag")]
    pub(super) include_deleted: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ActorQuery {
    pub(super) kind: Option<String>,
    #[serde(default, deserialize_with = "flag")]
    pub(super) include_deleted: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct SessionQuery {
    pub(super) workspace: Option<String>,
    pub(super) actor: Option<String>,
    pub(super) kind: Option<String>,
    #[serde(default, deserialize_with = "flag")]
    pub(super) current: bool,
    pub(super) start: Option<String>,
    pub(super) end: Option<String>,
    #[serde(default, rename = "subAgents", deserialize_with = "flag")]
    pub(super) sub_agents: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RecordQuery {
    pub(super) workspace: Option<String>,
    pub(super) actor: Option<String>,
    pub(super) session: Option<String>,
    pub(super) kind: Option<String>,
    pub(super) start: Option<String>,
    pub(super) end: Option<String>,
    pub(super) after: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MessageQuery {
    pub(super) workspace: Option<String>,
    pub(super) from: Option<String>,
    pub(super) to: Option<String>,
    pub(super) label: Option<String>,
    #[serde(default, deserialize_with = "flag")]
    pub(super) waiting: bool,
    pub(super) start: Option<String>,
    pub(super) end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MailboxQuery {
    pub(super) limit: Option<u64>,
    #[serde(default, deserialize_with = "flag")]
    pub(super) wait: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct EventQuery {
    pub(super) workspace: Option<String>,
}

pub(super) fn normalize_timestamp(
    value: Option<&str>,
    field: &str,
) -> Result<Option<String>, ApiError> {
    value
        .map(|value| {
            time::parse(value).map(time::format).map_err(|_| {
                error(
                    StatusCode::BAD_REQUEST,
                    &format!("range {field} must be an RFC 3339 timestamp"),
                )
            })
        })
        .transpose()
}

/// The `start` and `end` of a listing's range, normalized, with `start` not after `end`.
/// `subject` names what the listing lists in the refusal.
pub(super) fn time_range(
    start: Option<&str>,
    end: Option<&str>,
    subject: &str,
) -> Result<(Option<String>, Option<String>), ApiError> {
    let start = normalize_timestamp(start, "start")?;
    let end = normalize_timestamp(end, "end")?;
    if start
        .as_ref()
        .zip(end.as_ref())
        .is_some_and(|(start, end)| start > end)
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            &format!("{subject} range start must not be after end"),
        ));
    }
    Ok((start, end))
}
