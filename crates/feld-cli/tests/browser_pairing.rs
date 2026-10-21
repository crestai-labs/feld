//! Pairing and browser tokens (`docs/spec/design/auth.md`): the cookie and the key a pairing
//! code is exchanged for, the `Origin` check, the expiry, and the revocations.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Duration as Age, SecondsFormat, Utc};
use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

struct Fixture {
    temp: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            temp: tempfile::tempdir().expect("create temporary Feld home"),
        };
        support::install_plugins(fixture.home());
        fixture
    }

    fn home(&self) -> &Path {
        self.temp.path()
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .args(arguments);
        command
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().expect("run feld")
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let output = self.feld(arguments);
        assert!(
            output.status.success(),
            "feld {} failed:\nstdout: {}\nstderr: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("parse feld JSON output")
    }

    /// Starts the daemon and answers the client of its API.
    fn start(&self) -> Api {
        self.start_with(&[])
    }

    /// Starts the daemon with `arguments` and answers the client of its API.
    fn start_with(&self, arguments: &[&str]) -> Api {
        let started = self.json(&[&["daemon", "start", "--json"], arguments].concat());
        let api = started["daemon"]["api"].as_str().unwrap().to_owned();
        let port = api.rsplit_once(':').unwrap().1.to_owned();
        let token = fs::read_to_string(self.home().join("admin/api-token")).unwrap();
        Api {
            client: Client::new(),
            cookie: format!("feld_browser_{port}"),
            api,
            local: token.trim().to_owned(),
        }
    }

    fn stop(&self) {
        let output = self.feld(&["daemon", "stop"]);
        assert!(output.status.success(), "feld daemon stop failed");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
    }
}

struct Api {
    client: Client,
    api: String,
    cookie: String,
    local: String,
}

/// A browser token as the Web UI holds it: the value of its cookie and the key the page sends
/// in `X-Feld-Browser-Key`.
#[derive(Clone)]
struct Token {
    cookie: String,
    key: String,
}

impl Token {
    fn unknown() -> Self {
        Self {
            cookie: "unknown".to_owned(),
            key: "unknown".to_owned(),
        }
    }
}

impl Api {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.api)
    }

    fn local(&self, request: RequestBuilder) -> Response {
        request.bearer_auth(&self.local).send().unwrap()
    }

    /// `request` from the Web UI of the daemon's own address: the browser token's cookie and
    /// key, and the `Origin` of the page.
    fn browser(&self, request: RequestBuilder, token: &Token) -> Response {
        self.with_token(request, token)
            .header("Origin", &self.api)
            .send()
            .unwrap()
    }

    fn with_token(&self, request: RequestBuilder, token: &Token) -> RequestBuilder {
        request
            .header("Cookie", format!("{}={}", self.cookie, token.cookie))
            .header("X-Feld-Browser-Key", &token.key)
    }

    fn issue_code(&self, name: Option<&str>) -> Value {
        let request = self.client.post(self.url("/v1/ui/pairing-codes"));
        let request = match name {
            Some(name) => request.json(&json!({ "name": name })),
            None => request,
        };
        let response = self.local(request);
        assert_eq!(response.status(), StatusCode::OK);
        response.json().unwrap()
    }

    fn pair_request(&self, code: &str) -> RequestBuilder {
        self.client
            .post(self.url("/v1/ui/pair"))
            .json(&json!({ "code": code }))
    }

    /// Pairs with `code` from the daemon's own address and answers the browser token, with
    /// the pairing's response.
    fn pair(&self, code: &str) -> (Token, Value) {
        let response = self
            .pair_request(code)
            .header("Origin", &self.api)
            .send()
            .unwrap();
        self.paired(response)
    }

    fn paired(&self, response: Response) -> (Token, Value) {
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .to_owned();
        let (value, attributes) = cookie.split_once("; ").unwrap();
        assert_eq!(
            attributes,
            "HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000"
        );
        let cookie = value
            .strip_prefix(&format!("{}=", self.cookie))
            .unwrap()
            .to_owned();
        let paired: Value = response.json().unwrap();
        let key = paired["key"].as_str().unwrap().to_owned();
        assert_ne!(key, cookie);
        (Token { cookie, key }, paired)
    }

    fn browsers(&self) -> Vec<Value> {
        let response = self.local(self.client.get(self.url("/v1/ui/browsers")));
        assert_eq!(response.status(), StatusCode::OK);
        let listed: Value = response.json().unwrap();
        listed["browsers"].as_array().unwrap().clone()
    }

    fn status_as_browser(&self, token: &Token) -> StatusCode {
        self.browser(self.client.get(self.url("/v1/workspaces")), token)
            .status()
    }
}

/// Reads the event stream `response` to its end on a thread, and answers a receiver that
/// hears when the daemon closes it.
fn watch_stream(response: Response) -> Receiver<()> {
    assert_eq!(response.status(), StatusCode::OK);
    let (closed, receiver) = mpsc::channel();
    let mut response = response;
    thread::spawn(move || {
        let _ = response.read_to_end(&mut Vec::new());
        let _ = closed.send(());
    });
    receiver
}

fn assert_closes(stream: &Receiver<()>, what: &str) {
    assert_eq!(
        stream.recv_timeout(Duration::from_secs(5)),
        Ok(()),
        "{what} leaves its event stream open"
    );
}

fn assert_open(stream: &Receiver<()>, what: &str) {
    assert_eq!(
        stream.recv_timeout(Duration::from_millis(500)),
        Err(RecvTimeoutError::Timeout),
        "{what} closed"
    );
}

fn timestamp(age: Age) -> String {
    (Utc::now() - age).to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[test]
fn a_pairing_code_becomes_a_browser_token_that_calls_the_api_as_the_admin() {
    let fixture = Fixture::new();
    let api = fixture.start_with(&["--allow-host", PROXY]);

    let issued = api.issue_code(Some("laptop"));
    let code = issued["code"].as_str().unwrap();
    assert_eq!(issued["name"], "laptop");
    assert_eq!(code.len(), 9);
    assert_eq!(&code[4..5], "-");
    assert!(
        code.chars()
            .filter(|letter| *letter != '-')
            .all(|letter| "23456789ABCDEFGHJKMNPQRSTUVWXYZ".contains(letter)),
        "{code}"
    );
    assert!(issued["expiresAt"].as_str().unwrap() > issued_now().as_str());

    // A body, when there is one, is JSON with a printable name.
    let plain = api.local(
        api.client
            .post(api.url("/v1/ui/pairing-codes"))
            .header("Content-Type", "text/plain")
            .body(r#"{"name":"laptop"}"#),
    );
    assert_eq!(plain.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let control = api.local(
        api.client
            .post(api.url("/v1/ui/pairing-codes"))
            .json(&json!({ "name": "a\nb" })),
    );
    assert_eq!(control.status(), StatusCode::BAD_REQUEST);

    // The pairing checks `Origin` before it reads the code, so a refused one keeps the code.
    let missing_origin = api.pair_request(code).send().unwrap();
    assert_eq!(missing_origin.status(), StatusCode::FORBIDDEN);
    let other_origin = api
        .pair_request(code)
        .header("Origin", "http://evil.example")
        .send()
        .unwrap();
    assert_eq!(other_origin.status(), StatusCode::FORBIDDEN);

    // The code matches without regard to case and hyphens, and keeps nothing of the proxy
    // headers.
    let typed = code.replace('-', "").to_lowercase();
    let response = api
        .pair_request(&typed)
        .header("Origin", &api.api)
        .header("Tailscale-User-Login", "alice@example.com")
        .send()
        .unwrap();
    let (token, paired) = api.paired(response);
    let browser = &paired["browser"];
    assert!(browser["id"].as_str().unwrap().starts_with("browser_"));
    assert_eq!(browser["name"], "laptop");
    assert!(
        !paired.to_string().contains("alice@example.com"),
        "{paired}"
    );
    assert_eq!(browser["pairedAt"], browser["lastUsedAt"]);
    assert!(!paired.to_string().contains(&token.cookie));

    // A code is used once.
    let reused = api
        .pair_request(code)
        .header("Origin", &api.api)
        .send()
        .unwrap();
    assert_eq!(reused.status(), StatusCode::UNAUTHORIZED);

    // read, write, and SSE pass with the cookie and the key.
    assert_eq!(api.status_as_browser(&token), StatusCode::OK);
    let events = api.browser(api.client.get(api.url("/v1/events")), &token);
    assert_eq!(events.status(), StatusCode::OK);
    drop(events);
    let created = api.browser(
        api.client
            .post(api.url("/v1/workspaces"))
            .json(&json!({ "key": "from-browser" })),
        &token,
    );
    assert_eq!(created.status(), StatusCode::OK);

    // The caller of a write is the Admin.
    fixture.json(&[
        "actor", "create", "inbox", "--kind", "job", "--json", "--", "true",
    ]);
    let sent = api.browser(
        api.client
            .post(api.url("/v1/workspaces/default/actors/inbox/messages"))
            .json(&json!({ "body": "from the browser" })),
        &token,
    );
    assert_eq!(sent.status(), StatusCode::OK);
    let sent: Value = sent.json().unwrap();
    assert_eq!(sent["message"]["from"]["type"], "admin", "{sent}");

    // A write with the cookie must come from the address it is sent to.
    let write = || {
        api.with_token(api.client.post(api.url("/v1/workspaces")), &token)
            .json(&json!({ "key": "cross-site" }))
    };
    assert_eq!(write().send().unwrap().status(), StatusCode::FORBIDDEN);
    let host = api.api.strip_prefix("http://").unwrap();
    let port = host.rsplit_once(':').unwrap().1;
    for origin in [
        "http://evil.example".to_owned(),
        format!("http://127.0.0.1:{}", port.parse::<u16>().unwrap() + 1),
        format!("http://localhost:{port}"),
        "null".to_owned(),
    ] {
        let refused = write().header("Origin", &origin).send().unwrap();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN, "{origin}");
    }
    // Behind the proxy the request is sent to `X-Forwarded-Host`, on the default port of
    // the scheme when the port is left out.
    let proxied = write()
        .header("X-Forwarded-Host", "feld.example.ts.net")
        .header("Origin", "https://feld.example.ts.net")
        .send()
        .unwrap();
    assert_eq!(proxied.status(), StatusCode::OK);
    let proxied_port = write()
        .json(&json!({ "key": "on-the-port" }))
        .header("X-Forwarded-Host", "feld.example.ts.net:443")
        .header("Origin", "https://feld.example.ts.net")
        .send()
        .unwrap();
    assert_eq!(proxied_port.status(), StatusCode::OK);
    let proxied_other = write()
        .json(&json!({ "key": "other-host" }))
        .header("X-Forwarded-Host", "feld.example.ts.net")
        .header("Origin", &api.api)
        .send()
        .unwrap();
    assert_eq!(proxied_other.status(), StatusCode::FORBIDDEN);

    // The browser token travels in the cookie and the key only, and `Authorization` alone
    // decides the caller when it is there.
    for bearer in [&token.cookie, &token.key] {
        let bearer = api
            .client
            .get(api.url("/v1/workspaces"))
            .bearer_auth(bearer)
            .send()
            .unwrap();
        assert_eq!(bearer.status(), StatusCode::UNAUTHORIZED);
    }
    let both = api
        .with_token(api.client.get(api.url("/v1/workspaces")), &token)
        .bearer_auth("not-a-token")
        .send()
        .unwrap();
    assert_eq!(both.status(), StatusCode::UNAUTHORIZED);

    // The Runtime API and the management of pairing refuse the browser token.
    let self_read = api.browser(api.client.get(api.url("/v1/self")), &token);
    assert_eq!(self_read.status(), StatusCode::FORBIDDEN);
    for request in [
        api.client.post(api.url("/v1/ui/pairing-codes")),
        api.client.get(api.url("/v1/ui/browsers")),
        api.client.delete(api.url(&format!(
            "/v1/ui/browsers/{}",
            browser["id"].as_str().unwrap()
        ))),
    ] {
        assert_eq!(api.browser(request, &token).status(), StatusCode::FORBIDDEN);
    }
    // Unpairing takes the browser token alone.
    let unpair_local = api.local(api.client.post(api.url("/v1/ui/unpair")));
    assert_eq!(unpair_local.status(), StatusCode::FORBIDDEN);

    // Without a valid token the pairing management answers 401.
    let anonymous = api.client.get(api.url("/v1/ui/browsers")).send().unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let stranger = api.browser(api.client.get(api.url("/v1/workspaces")), &Token::unknown());
    assert_eq!(stranger.status(), StatusCode::UNAUTHORIZED);

    let listed = api.browsers();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], browser["id"]);
    assert!(!format!("{listed:?}").contains("alice@example.com"));
    assert!(listed[0].get("revokedAt").is_none());

    // The token outlives the daemon.
    fixture.stop();
    let api = fixture.start_with(&["--allow-host", PROXY]);
    assert_eq!(api.status_as_browser(&token), StatusCode::OK);
}

#[test]
fn the_cookie_a_server_on_another_port_receives_calls_nothing() {
    let fixture = Fixture::new();
    let api = fixture.start();
    let (token, _) = api.pair(api.issue_code(None)["code"].as_str().unwrap());
    let (other, _) = api.pair(api.issue_code(None)["code"].as_str().unwrap());

    // The browser sends the cookie to every port of the host, but the key stays in the
    // storage of the daemon's origin: the cookie alone, the key alone, and the cookie with
    // the key of another browser token are all refused.
    let cookie_only = |request: RequestBuilder| {
        request
            .header("Cookie", format!("{}={}", api.cookie, token.cookie))
            .header("Origin", &api.api)
            .send()
            .unwrap()
            .status()
    };
    assert_eq!(
        cookie_only(api.client.get(api.url("/v1/workspaces"))),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        cookie_only(api.client.get(api.url("/v1/events"))),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        cookie_only(
            api.client
                .post(api.url("/v1/workspaces"))
                .json(&json!({ "key": "stolen" }))
        ),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        cookie_only(api.client.post(api.url("/v1/ui/unpair"))),
        StatusCode::UNAUTHORIZED
    );
    let key_only = api
        .client
        .get(api.url("/v1/workspaces"))
        .header("X-Feld-Browser-Key", &token.key)
        .send()
        .unwrap();
    assert_eq!(key_only.status(), StatusCode::UNAUTHORIZED);
    let mixed = Token {
        cookie: token.cookie.clone(),
        key: other.key.clone(),
    };
    assert_eq!(api.status_as_browser(&mixed), StatusCode::UNAUTHORIZED);

    // A pairing with the cookie alone replaces nothing.
    let response = api
        .pair_request(api.issue_code(None)["code"].as_str().unwrap())
        .header("Cookie", format!("{}={}", api.cookie, token.cookie))
        .header("Origin", &api.api)
        .send()
        .unwrap();
    api.paired(response);
    assert_eq!(api.status_as_browser(&token), StatusCode::OK);
    assert_eq!(api.status_as_browser(&other), StatusCode::OK);
}

#[test]
fn a_cookie_of_the_same_name_ahead_of_the_browser_token_hides_nothing() {
    let fixture = Fixture::new();
    let api = fixture.start();
    let (token, _) = api.pair(api.issue_code(None)["code"].as_str().unwrap());

    // A server on another port of the host can set a cookie of the same name with a longer
    // path, which the browser sends first.
    let planted = |request: RequestBuilder, token: &Token| {
        request
            .header(
                "Cookie",
                format!("{0}=planted; {0}={1}", api.cookie, token.cookie),
            )
            .header("X-Feld-Browser-Key", &token.key)
            .header("Origin", &api.api)
            .send()
            .unwrap()
    };
    assert_eq!(
        planted(api.client.get(api.url("/v1/workspaces")), &token).status(),
        StatusCode::OK
    );

    // Pairing again behind the planted cookie still replaces the token.
    let response = planted(
        api.pair_request(api.issue_code(None)["code"].as_str().unwrap()),
        &token,
    );
    let (second, _) = api.paired(response);
    assert_eq!(api.status_as_browser(&token), StatusCode::UNAUTHORIZED);
    assert_eq!(
        planted(api.client.get(api.url("/v1/workspaces")), &second).status(),
        StatusCode::OK
    );
    assert_eq!(api.browsers().len(), 1);
}

/// The host name of the HTTPS proxy the tests allow.
const PROXY: &str = "feld.example.ts.net";

fn issued_now() -> String {
    timestamp(Age::zero())
}

#[test]
fn a_session_token_cannot_manage_pairing() {
    let fixture = Fixture::new();
    let api = fixture.start();
    let written = fixture.home().join("session-token");
    let script = format!(
        "printf %s \"$FELD_SESSION_TOKEN\" > '{}'; sleep 30",
        written.display()
    );
    fixture.json(&[
        "actor", "create", "holder", "--kind", "job", "--json", "--", "sh", "-c", &script,
    ]);
    fixture.json(&["actor", "start", "holder", "--json"]);
    let deadline = Instant::now() + Duration::from_secs(10);
    let session = loop {
        if let Ok(token) = fs::read_to_string(&written)
            && !token.is_empty()
        {
            break token;
        }
        assert!(Instant::now() < deadline, "the job did not write its token");
        thread::sleep(Duration::from_millis(50));
    };

    for request in [
        api.client.post(api.url("/v1/ui/pairing-codes")),
        api.client.get(api.url("/v1/ui/browsers")),
        api.client.delete(api.url("/v1/ui/browsers/browser_0")),
        api.client.post(api.url("/v1/ui/unpair")),
    ] {
        let refused = request.bearer_auth(&session).send().unwrap();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    }
    let _ = fixture.feld(&["session", "kill", "holder"]);
}

#[test]
fn browser_cookie_is_plain_on_loopback_http_and_secure_behind_https_proxy() {
    let fixture = Fixture::new();
    let api = fixture.start_with(&["--allow-host", PROXY]);

    let direct = api.issue_code(Some("safari"));
    let response = api
        .pair_request(direct["code"].as_str().unwrap())
        .header("Origin", &api.api)
        .send()
        .unwrap();
    let direct_cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert_eq!(
        direct_cookie.split_once("; ").unwrap().1,
        "HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000"
    );

    let proxied = api.issue_code(Some("phone"));
    let response = api
        .pair_request(proxied["code"].as_str().unwrap())
        .header("X-Forwarded-Host", "feld.example.ts.net")
        .header("Origin", "https://feld.example.ts.net")
        .send()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let (value, attributes) = cookie.split_once("; ").unwrap();
    assert_eq!(
        attributes,
        "HttpOnly; SameSite=Strict; Secure; Path=/; Max-Age=2592000"
    );
    let token = Token {
        cookie: value
            .strip_prefix(&format!("{}=", api.cookie))
            .unwrap()
            .to_owned(),
        key: response.json::<Value>().unwrap()["key"]
            .as_str()
            .unwrap()
            .to_owned(),
    };

    let unpaired = api
        .with_token(api.client.post(api.url("/v1/ui/unpair")), &token)
        .header("X-Forwarded-Host", "feld.example.ts.net")
        .header("Origin", "https://feld.example.ts.net")
        .send()
        .unwrap();
    assert_eq!(unpaired.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        unpaired.headers()["set-cookie"],
        format!(
            "{}=; HttpOnly; SameSite=Strict; Secure; Path=/; Max-Age=0",
            api.cookie
        )
        .as_str()
    );
}

#[test]
fn pairing_again_unpairing_and_revoking_end_a_browser_token() {
    let fixture = Fixture::new();
    let api = fixture.start();

    // Pairing again from the same browser replaces its token.
    let (first, _) = api.pair(api.issue_code(None)["code"].as_str().unwrap());
    let again = api.issue_code(None);
    assert_eq!(again["name"], "browser");
    let response = api.browser(api.pair_request(again["code"].as_str().unwrap()), &first);
    let (second, _) = api.paired(response);
    assert_eq!(api.status_as_browser(&first), StatusCode::UNAUTHORIZED);
    assert_eq!(api.status_as_browser(&second), StatusCode::OK);
    assert_eq!(api.browsers().len(), 1);

    // Unpairing revokes the token and clears the cookie.
    let unpaired = api.browser(api.client.post(api.url("/v1/ui/unpair")), &second);
    assert_eq!(unpaired.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        unpaired.headers()["set-cookie"],
        format!(
            "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
            api.cookie
        )
        .as_str()
    );
    assert_eq!(api.status_as_browser(&second), StatusCode::UNAUTHORIZED);
    let unpaired_again = api.browser(api.client.post(api.url("/v1/ui/unpair")), &second);
    assert_eq!(unpaired_again.status(), StatusCode::UNAUTHORIZED);
    assert!(api.browsers().is_empty());

    // The local token revokes a browser token by its `id`.
    let (token, paired) = api.pair(api.issue_code(Some("phone"))["code"].as_str().unwrap());
    let id = paired["browser"]["id"].as_str().unwrap();
    let revoked = api.local(api.client.delete(api.url(&format!("/v1/ui/browsers/{id}"))));
    assert_eq!(revoked.status(), StatusCode::OK);
    let revoked: Value = revoked.json().unwrap();
    assert_eq!(revoked["browser"]["id"], id);
    assert_eq!(revoked["browser"]["name"], "phone");
    assert!(revoked["browser"]["revokedAt"].is_string());
    assert_eq!(api.status_as_browser(&token), StatusCode::UNAUTHORIZED);
    let missing = api.local(api.client.delete(api.url(&format!("/v1/ui/browsers/{id}"))));
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[test]
fn ending_a_browser_token_closes_the_event_streams_it_opened() {
    let fixture = Fixture::new();
    let api = fixture.start();
    let events =
        |token: &Token| watch_stream(api.browser(api.client.get(api.url("/v1/events")), token));
    let local = watch_stream(api.local(api.client.get(api.url("/v1/events"))));
    let (other, _) = api.pair(api.issue_code(None)["code"].as_str().unwrap());
    let other_stream = events(&other);

    // Pairing again from the same browser.
    let (first, _) = api.pair(api.issue_code(None)["code"].as_str().unwrap());
    let stream = events(&first);
    assert_open(&stream, "a valid browser token's stream");
    let response = api.browser(
        api.pair_request(api.issue_code(None)["code"].as_str().unwrap()),
        &first,
    );
    let (second, _) = api.paired(response);
    assert_closes(&stream, "pairing again");

    // Unpairing.
    let stream = events(&second);
    let unpaired = api.browser(api.client.post(api.url("/v1/ui/unpair")), &second);
    assert_eq!(unpaired.status(), StatusCode::NO_CONTENT);
    assert_closes(&stream, "unpairing");

    // Revoking by the local token.
    let (token, paired) = api.pair(api.issue_code(None)["code"].as_str().unwrap());
    let stream = events(&token);
    let id = paired["browser"]["id"].as_str().unwrap();
    let revoked = api.local(api.client.delete(api.url(&format!("/v1/ui/browsers/{id}"))));
    assert_eq!(revoked.status(), StatusCode::OK);
    assert_closes(&stream, "revoking");
    let reopened = api.browser(api.client.get(api.url("/v1/events")), &token);
    assert_eq!(reopened.status(), StatusCode::UNAUTHORIZED);

    // The streams of another browser token and of the local token stay open.
    assert_open(&other_stream, "another browser token's stream");
    assert_open(&local, "the local token's stream");
}

#[test]
fn five_failed_pairings_in_a_row_drop_every_pending_code() {
    let fixture = Fixture::new();
    let api = fixture.start();
    let attempt = |code: &str| {
        api.pair_request(code)
            .header("Origin", &api.api)
            .send()
            .unwrap()
            .status()
    };

    // A success counts again from zero.
    let kept = api.issue_code(None);
    for _ in 0..4 {
        assert_eq!(attempt("WRONG-CODE"), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(attempt(kept["code"].as_str().unwrap()), StatusCode::OK);
    for _ in 0..4 {
        assert_eq!(attempt("WRONG-CODE"), StatusCode::UNAUTHORIZED);
    }

    let dropped = api.issue_code(None);
    let other = api.issue_code(None);
    assert_eq!(attempt("WRONG-CODE"), StatusCode::UNAUTHORIZED);
    assert_eq!(
        attempt(dropped["code"].as_str().unwrap()),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        attempt(other["code"].as_str().unwrap()),
        StatusCode::UNAUTHORIZED
    );

    // A code issued afterwards pairs.
    let fresh = api.issue_code(None);
    assert_eq!(attempt(fresh["code"].as_str().unwrap()), StatusCode::OK);
}

#[test]
fn a_browser_token_lapses_seven_days_after_its_last_use_and_thirty_after_its_pairing() {
    let fixture = Fixture::new();
    let api = fixture.start();
    let pair = |name: &str| {
        let (token, paired) = api.pair(api.issue_code(Some(name))["code"].as_str().unwrap());
        (token, paired["browser"]["id"].as_str().unwrap().to_owned())
    };
    let (idle, idle_id) = pair("idle");
    let (old, old_id) = pair("old");
    let (used, used_id) = pair("used");
    let (fresh, _) = pair("fresh");
    fixture.stop();

    // Age the records as days passing would.
    let store = rusqlite::Connection::open(fixture.home().join("admin/feld.db")).unwrap();
    let age = |id: &str, paired: Age, used: Age| {
        store
            .execute(
                "UPDATE browser_tokens SET paired_at = ?2, last_used_at = ?3 WHERE id = ?1",
                rusqlite::params![id, timestamp(paired), timestamp(used)],
            )
            .unwrap();
    };
    age(&idle_id, Age::days(8), Age::days(7) + Age::minutes(1));
    age(&old_id, Age::days(30) + Age::minutes(1), Age::hours(1));
    age(&used_id, Age::days(29), Age::days(6));
    drop(store);

    let api = fixture.start();
    assert_eq!(api.status_as_browser(&idle), StatusCode::UNAUTHORIZED);
    assert_eq!(api.status_as_browser(&old), StatusCode::UNAUTHORIZED);
    assert_eq!(api.status_as_browser(&fresh), StatusCode::OK);

    // A use pushes the idle limit, and the pairing bounds it.
    assert_eq!(api.status_as_browser(&used), StatusCode::OK);
    let listed = api.browsers();
    let names: Vec<_> = listed.iter().map(|browser| &browser["name"]).collect();
    assert_eq!(names, ["used", "fresh"]);
    let used_listed = &listed[0];
    let last_used = used_listed["lastUsedAt"].as_str().unwrap();
    assert!(
        last_used > timestamp(Age::minutes(1)).as_str(),
        "{used_listed}"
    );
    let paired_at =
        chrono::DateTime::parse_from_rfc3339(used_listed["pairedAt"].as_str().unwrap()).unwrap();
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(used_listed["expiresAt"].as_str().unwrap()).unwrap(),
        paired_at + Age::days(30)
    );

    // An expired token is no longer revocable.
    let expired = api.local(
        api.client
            .delete(api.url(&format!("/v1/ui/browsers/{idle_id}"))),
    );
    assert_eq!(expired.status(), StatusCode::NOT_FOUND);
}

#[test]
fn a_rebound_host_is_refused_before_it_can_drop_the_pairing_codes() {
    let fixture = Fixture::new();
    let api = fixture.start();
    let port = api.api.rsplit_once(':').unwrap().1.to_owned();
    let rebound = format!("evil.example:{port}");
    let code = api.issue_code(None);

    // A page DNS rebinding points at the daemon sends its own name in `Host` and `Origin`.
    for _ in 0..5 {
        let refused = api
            .pair_request("WRONG-CODE")
            .header("Host", &rebound)
            .header("Origin", format!("http://{rebound}"))
            .send()
            .unwrap();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            refused.headers()["content-type"],
            "text/plain; charset=utf-8"
        );
        let body = refused.text().unwrap();
        assert!(body.contains("\"evil.example\""), "{body}");
        assert!(
            body.contains("feld daemon restart --allow-host evil.example"),
            "{body}"
        );
    }
    // `X-Forwarded-Host` is checked as well as `Host`, either way round.
    for (host, forwarded) in [
        (rebound.clone(), format!("127.0.0.1:{port}")),
        (format!("127.0.0.1:{port}"), rebound.clone()),
    ] {
        let refused = api
            .pair_request("WRONG-CODE")
            .header("Host", &host)
            .header("X-Forwarded-Host", &forwarded)
            .header("Origin", format!("http://{forwarded}"))
            .send()
            .unwrap();
        assert_eq!(
            refused.status(),
            StatusCode::FORBIDDEN,
            "{host} {forwarded}"
        );
    }
    // None of them counted as a failed pairing.
    let (token, _) = api.pair(code["code"].as_str().unwrap());

    // The Web UI and the API answer the loopback names on the daemon's port alone.
    for host in [format!("127.0.0.1:{port}"), format!("localhost:{port}")] {
        let page = api
            .client
            .get(api.url("/ui/"))
            .header("Host", &host)
            .send()
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK, "{host}");
        let read = api.browser(
            api.client
                .get(api.url("/v1/workspaces"))
                .header("Host", &host),
            &token,
        );
        assert_eq!(read.status(), StatusCode::OK, "{host}");
    }
    let page = api
        .client
        .get(api.url("/ui/"))
        .header("Host", &rebound)
        .send()
        .unwrap();
    assert_eq!(page.status(), StatusCode::FORBIDDEN);
    let other_port = format!("localhost:{}", port.parse::<u16>().unwrap() + 1);
    let page = api
        .client
        .get(api.url("/ui/"))
        .header("Host", &other_port)
        .send()
        .unwrap();
    assert_eq!(page.status(), StatusCode::FORBIDDEN);
    let body = page.text().unwrap();
    assert!(
        body.contains(&format!("http://127.0.0.1:{port}/ui/")),
        "{body}"
    );
}

#[test]
fn the_allowed_host_names_are_shown_and_kept_across_a_restart() {
    let fixture = Fixture::new();
    let api = fixture.start_with(&["--allow-host", "Feld.Example.ts.net"]);
    let status = |host: &str| {
        api.client
            .get(api.url("/v1/daemon/status"))
            .header("Host", host)
            .send()
            .unwrap()
            .status()
    };
    // An allowed name is answered on any port, and reaches the authentication.
    assert_eq!(status(PROXY), StatusCode::UNAUTHORIZED);
    assert_eq!(status(&format!("{PROXY}:8443")), StatusCode::UNAUTHORIZED);
    assert_eq!(status("other.example"), StatusCode::FORBIDDEN);

    let shown = fixture.json(&["daemon", "status", "--json"]);
    assert_eq!(shown["allowedHosts"], json!([PROXY]));
    let text = fixture.feld(&["daemon", "status"]);
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains(&format!("  allow_host: {PROXY}\n")), "{text}");

    // A running daemon answers a start that names another host with the way to add it.
    let refused = fixture.feld(&["daemon", "start", "--allow-host", "other.example"]);
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("feld daemon restart --allow-host other.example"),
        "{stderr}"
    );
    let invalid = fixture.feld(&["daemon", "start", "--allow-host", "https://other.example"]);
    assert!(!invalid.status.success());

    // A restart keeps the names of the daemon it stops and adds those it names.
    let restarted = fixture.json(&["daemon", "restart", "--json"]);
    assert_eq!(restarted["daemon"]["allowedHosts"], json!([PROXY]));
    let restarted = fixture.json(&[
        "daemon",
        "restart",
        "--json",
        "--allow-host",
        "other.example",
    ]);
    assert_eq!(
        restarted["daemon"]["allowedHosts"],
        json!([PROXY, "other.example"])
    );
    assert_eq!(status("other.example"), StatusCode::UNAUTHORIZED);
}
