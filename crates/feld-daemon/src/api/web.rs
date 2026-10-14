use std::borrow::Cow;

use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use rust_embed::RustEmbed;

use super::routes::Routes;

const HTML: &str = "text/html; charset=utf-8";

/// The Web UI: the Vite build in `web/dist` (see `web/README.md`), embedded whole. Its file
/// names carry no hash, so every file is revalidated on every request against an `ETag` of
/// its content, and a new binary serves its own build at once.
#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct Assets;

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => HTML,
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn asset_by_path(path: &str, if_none_match: Option<&HeaderValue>) -> Option<Response> {
    let file = Assets::get(path)?;
    let etag = etag(&file.metadata.sha256_hash());
    let mut response = asset_response(file.data, content_type(path), &etag);
    if if_none_match.is_some_and(|value| matches_etag(value, &etag)) {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        *response.body_mut() = Default::default();
    }
    Some(response)
}

fn etag(sha256: &[u8; 32]) -> String {
    let hex: String = sha256.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("\"{hex}\"")
}

/// `If-None-Match` compares weakly (RFC 9110 13.1.2): a `W/` prefix does not matter.
fn matches_etag(if_none_match: &HeaderValue, etag: &str) -> bool {
    let Ok(value) = if_none_match.to_str() else {
        return false;
    };
    value.split(',').map(str::trim).any(|candidate| {
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    })
}

pub fn routes<S>() -> Routes<S>
where
    S: Clone + Send + Sync + 'static,
{
    Routes::new()
        .route(Method::GET, "/", || async { Redirect::temporary("/ui/") })
        .route(Method::GET, "/ui", |RawQuery(query): RawQuery| async move {
            page_redirect(query)
        })
        .route(Method::GET, "/ui/", |headers: HeaderMap| async move {
            asset(String::new(), &headers)
        })
        .route(
            Method::GET,
            "/ui/{*path}",
            |Path(path): Path<String>, headers: HeaderMap| async move { asset(path, &headers) },
        )
}

/// The page lives at `/ui/`; the query it was asked with names what it shows (the
/// Workspace, the inspected Session), so the redirect keeps it.
fn page_redirect(query: Option<String>) -> Redirect {
    match query {
        Some(query) if !query.is_empty() => Redirect::temporary(&format!("/ui/?{query}")),
        _ => Redirect::temporary("/ui/"),
    }
}

/// A file of the build, or the page itself for a path of the page's own routes
/// (`/ui/daemon`, `/ui/settings`), which name no file.
fn asset(path: String, headers: &HeaderMap) -> Response {
    if path.split('/').any(|segment| segment == "..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let if_none_match = headers.get(header::IF_NONE_MATCH);
    asset_by_path(&path, if_none_match)
        .or_else(|| {
            let last = path.rsplit('/').next().unwrap_or_default();
            (!last.contains('.'))
                .then(|| asset_by_path("index.html", if_none_match))
                .flatten()
        })
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn asset_response(bytes: Cow<'static, [u8]>, content_type: &str, etag: &str) -> Response {
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).expect("asset content types are valid headers"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(etag).expect("a quoted hex digest is a valid header"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            // The Actors page sizes its lanes, bands, and SVG with style attributes on the
            // elements it renders, so inline styles are allowed, and the stylesheets draw
            // control glyphs as data: SVG. Scripts are not: every script is served from
            // this origin as a file.
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logo() -> String {
        Assets::iter()
            .find(|path| path.starts_with("assets/feld-logo-dark-") && path.ends_with(".png"))
            .expect("the build has the dark logo")
            .into_owned()
    }

    fn get(path: &str, if_none_match: Option<&str>) -> Response {
        let mut headers = HeaderMap::new();
        if let Some(value) = if_none_match {
            headers.insert(header::IF_NONE_MATCH, HeaderValue::from_str(value).unwrap());
        }
        asset(path.to_owned(), &headers)
    }

    #[test]
    fn images_are_revalidated_like_every_other_file() {
        let response = get(&logo(), None);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(response.headers().contains_key(header::ETAG));
    }

    #[test]
    fn a_matching_if_none_match_is_answered_with_not_modified() {
        let etag = get(&logo(), None).headers()[header::ETAG].clone();
        let etag = etag.to_str().unwrap();
        for if_none_match in [
            etag.to_owned(),
            format!("W/{etag}"),
            format!("\"x\", {etag}"),
            "*".to_owned(),
        ] {
            let response = get(&logo(), Some(&if_none_match));
            assert_eq!(
                response.status(),
                StatusCode::NOT_MODIFIED,
                "{if_none_match}"
            );
            assert_eq!(response.headers()[header::ETAG], etag);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        }
    }

    #[test]
    fn another_etag_is_answered_with_the_file() {
        let etag = get(&logo(), None).headers()[header::ETAG].clone();
        let other = get("app.css", None).headers()[header::ETAG].clone();
        assert_ne!(etag, other);
        let response = get(&logo(), Some(other.to_str().unwrap()));
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn page_routes_share_the_etag_of_the_page() {
        let page = get("", None).headers()[header::ETAG].clone();
        let response = get("daemon", Some(page.to_str().unwrap()));
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }
}
