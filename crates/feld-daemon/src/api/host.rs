//! The host names the daemon answers (`docs/spec/design/auth.md`): the loopback names on its
//! own port, and the names its user allowed with `--allow-host`.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::auth::authority;
use super::{AppState, Route};

const LOOPBACK: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

/// The host names the daemon answers.
#[derive(Clone, Debug)]
pub struct AllowedHosts {
    /// The port the API listens on, which a loopback name must carry.
    port: u16,
    /// The names allowed with `--allow-host`, on any port.
    names: Vec<String>,
}

impl AllowedHosts {
    pub fn new(port: u16, names: Vec<String>) -> Self {
        Self { port, names }
    }

    /// Why the daemon does not answer `value` (`host[:port]`), and `None` when it does.
    fn refused<'a>(&self, value: &'a str) -> Option<Refused<'a>> {
        let Some((name, port)) = authority(value, 80) else {
            return Some(Refused::Unreadable(value));
        };
        if LOOPBACK.contains(&name.as_str()) {
            (port != self.port).then_some(Refused::Loopback)
        } else {
            (!self.names.contains(&name)).then_some(Refused::Name(name))
        }
    }

    /// The first host of `headers` the daemon does not answer: `Host` (the authority of
    /// the URI without it) and each host of `X-Forwarded-Host`.
    fn check<'a>(
        &self,
        headers: &'a HeaderMap,
        uri_authority: Option<&'a str>,
    ) -> Option<Refused<'a>> {
        let hosts: Vec<&str> = headers
            .get_all(header::HOST)
            .iter()
            .map(|value| value.to_str().unwrap_or_default())
            .collect();
        let hosts = if hosts.is_empty() {
            vec![uri_authority.unwrap_or_default()]
        } else {
            hosts
        };
        let forwarded = headers
            .get_all("x-forwarded-host")
            .iter()
            .flat_map(|value| value.to_str().unwrap_or_default().split(','));
        hosts
            .into_iter()
            .chain(forwarded)
            .find_map(|value| self.refused(value.trim()))
    }
}

/// Why a host is refused.
enum Refused<'a> {
    /// A name not allowed.
    Name(String),
    /// A loopback name on a port other than the daemon's.
    Loopback,
    /// A value that names no host.
    Unreadable(&'a str),
}

/// A request sent to a host the daemon does not answer is refused before it is read, with
/// the way to allow the host, as a person reads it in the browser.
pub(super) async fn check_host(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    // The host names guard the listener; a request of the spool came through none.
    if request.extensions().get::<Route>() == Some(&Route::Spool) {
        return next.run(request).await;
    }
    let refused = state.hosts.check(
        request.headers(),
        request
            .uri()
            .authority()
            .map(|authority| authority.as_str()),
    );
    let Some(refused) = refused else {
        return next.run(request).await;
    };
    let port = state.hosts.port;
    let body = match refused {
        Refused::Name(name) => format!(
            "Feld does not answer the host name \"{name}\".\n\n\
             To open Feld under this name, allow it on the machine where the daemon runs:\n\n  \
             feld daemon restart --allow-host {name}\n"
        ),
        Refused::Loopback => format!(
            "Feld listens on port {port}. Open http://127.0.0.1:{port}/ui/ on the machine where the daemon runs.\n"
        ),
        Refused::Unreadable(value) => format!(
            "Feld does not answer the host \"{}\".\n",
            value.escape_debug()
        ),
    };
    (
        StatusCode::FORBIDDEN,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

/// `value` as a name `--allow-host` takes: a host name or an IP address, without a scheme,
/// a port or a path, in lowercase.
pub fn allowed_host_name(value: &str) -> Result<String, String> {
    let name = value.trim().to_ascii_lowercase();
    let valid = match name
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        Some(ipv6) => ipv6.parse::<std::net::Ipv6Addr>().is_ok(),
        None => {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        }
    };
    if valid {
        Ok(name)
    } else {
        Err(format!(
            "'{value}' is not a host name; give the name alone, without a scheme, a port or a path (e.g. feld.example.ts.net)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(*name, value.parse().unwrap());
        }
        headers
    }

    fn refused(hosts: &AllowedHosts, pairs: &[(&'static str, &str)]) -> bool {
        hosts.check(&headers(pairs), None).is_some()
    }

    #[test]
    fn loopback_names_on_the_daemon_port_and_allowed_names_are_answered() {
        let hosts = AllowedHosts::new(7381, vec!["feld.example.ts.net".to_owned()]);
        for host in [
            "127.0.0.1:7381",
            "localhost:7381",
            "[::1]:7381",
            "LOCALHOST:7381",
        ] {
            assert!(!refused(&hosts, &[("host", host)]), "{host}");
        }
        for host in [
            "127.0.0.1:7382",
            "localhost",
            "[::1]:80",
            "evil.example:7381",
            "",
        ] {
            assert!(refused(&hosts, &[("host", host)]), "{host}");
        }
        for host in ["feld.example.ts.net", "feld.example.ts.net:8443"] {
            assert!(!refused(&hosts, &[("host", host)]), "{host}");
        }
    }

    #[test]
    fn every_forwarded_host_must_be_answered_too() {
        let hosts = AllowedHosts::new(7381, vec!["feld.example.ts.net".to_owned()]);
        assert!(!refused(
            &hosts,
            &[
                ("host", "127.0.0.1:7381"),
                ("x-forwarded-host", "feld.example.ts.net")
            ]
        ));
        assert!(refused(
            &hosts,
            &[
                ("host", "evil.example:7381"),
                ("x-forwarded-host", "127.0.0.1:7381")
            ]
        ));
        assert!(refused(
            &hosts,
            &[
                ("host", "127.0.0.1:7381"),
                ("x-forwarded-host", "feld.example.ts.net, evil.example")
            ]
        ));
        assert!(refused(
            &hosts,
            &[("host", "127.0.0.1:7381"), ("host", "evil.example:7381")]
        ));
    }

    #[test]
    fn a_request_without_host_falls_back_to_the_uri() {
        let hosts = AllowedHosts::new(7381, Vec::new());
        assert!(
            hosts
                .check(&HeaderMap::new(), Some("127.0.0.1:7381"))
                .is_none()
        );
        assert!(
            hosts
                .check(&HeaderMap::new(), Some("evil.example:7381"))
                .is_some()
        );
        assert!(hosts.check(&HeaderMap::new(), None).is_some());
    }

    #[test]
    fn allow_host_takes_a_name_alone() {
        assert_eq!(
            allowed_host_name("Feld.Example.ts.net").as_deref(),
            Ok("feld.example.ts.net")
        );
        assert_eq!(allowed_host_name("100.64.0.1").as_deref(), Ok("100.64.0.1"));
        assert_eq!(allowed_host_name("[fd7a::1]").as_deref(), Ok("[fd7a::1]"));
        for value in [
            "",
            "https://feld.example.ts.net",
            "feld.example.ts.net:443",
            "feld.example.ts.net/ui",
            "a b",
            "fd7a::1",
        ] {
            assert!(allowed_host_name(value).is_err(), "{value}");
        }
    }
}
