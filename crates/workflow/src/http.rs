//! HTTP client shared by the forge implementations (JSON requests, no automatic error status).

use std::time::Duration;

use serde_json::Value;
use susi_core::tf;

use crate::{Error, Result};

pub struct Http {
    agent: ureq::Agent,
}

/// Answer of the server.
pub struct Response {
    pub status: u16,
    /// Parsed JSON (`Null` for an empty body, a string for non-JSON text).
    pub body: Value,
    /// Raw text, for error messages.
    pub text: String,
    /// Selected headers in lower case (`x-ratelimit-remaining`, …).
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// Response headers kept for the forges.
const KEPT_HEADERS: [&str; 3] = ["x-ratelimit-remaining", "x-ratelimit-reset", "x-github-sso"];

impl Http {
    pub fn new() -> Self {
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .tls_config(tls)
            .user_agent(concat!("susi/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self { agent }
    }

    /// Sends a request; `server` names the server in connection errors.
    pub fn send(&self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<&Value>, server: &str) -> Result<Response> {
        macro_rules! with_headers {
            ($r:expr) => {{
                let mut r = $r;
                for (k, v) in headers {
                    r = r.header(*k, *v);
                }
                r
            }};
        }
        let data = body.map(Value::to_string).unwrap_or_default();
        let result = match method {
            "GET" => with_headers!(self.agent.get(url)).call(),
            "DELETE" => with_headers!(self.agent.delete(url)).call(),
            "PUT" => with_headers!(self.agent.put(url)).header("Content-Type", "application/json").send(data),
            "PATCH" => with_headers!(self.agent.patch(url)).header("Content-Type", "application/json").send(data),
            _ => with_headers!(self.agent.post(url)).header("Content-Type", "application/json").send(data),
        };
        let mut resp = result.map_err(|e| Error::Forge(tf!("Connection to {url} failed: {error}", url = server, error = e)))?;
        let status = resp.status().as_u16();
        let headers =
            KEPT_HEADERS.iter().filter_map(|k| resp.headers().get(*k).and_then(|v| v.to_str().ok()).map(|v| (k.to_string(), v.to_string()))).collect();
        let text = resp.body_mut().read_to_string().map_err(|e| Error::Forge(e.to_string()))?;
        let body = if text.trim().is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone())) };
        Ok(Response { status, body, text, headers })
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

/// URL query component encoding (RFC 3986 unreserved characters stay).
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Minimal HTTP server for the forge tests.
#[cfg(test)]
pub(crate) mod test_server {
    use std::io::{BufRead, BufReader, Read, Write};

    /// A received request: first line, headers and body.
    pub struct Request {
        pub line: String,
        pub head: String,
        pub body: String,
    }

    impl Request {
        pub fn json(&self) -> serde_json::Value {
            serde_json::from_str(&self.body).unwrap_or_default()
        }
    }

    /// Answers `count` requests with `answer(first line) -> (status, extra headers, body)`; returns
    /// the server URL and a handle yielding the received requests.
    pub fn serve(count: usize, answer: fn(&str) -> (u16, &'static str, String)) -> (String, std::thread::JoinHandle<Vec<Request>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for stream in listener.incoming().take(count) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    head.push_str(&line);
                }
                let len = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let line = head.lines().next().unwrap_or_default().to_string();
                let (status, extra, text) = answer(&line);
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                )
                .unwrap();
                seen.push(Request { line, head, body: String::from_utf8_lossy(&body).to_string() });
            }
            seen
        });
        (url, handle)
    }
}
