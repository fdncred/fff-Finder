//! Talking to a running fffinder: used to hand a second launch to the
//! window that is already open, and by tests.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::{Endpoint, http};

/// The app's server, from its `server.json`, if there is one.
pub fn discover() -> Option<Endpoint> {
    discover_in(&super::run_dir())
}

/// The server described in `run_dir/server.json`.
pub fn discover_in(run_dir: &Path) -> Option<Endpoint> {
    let text = std::fs::read_to_string(run_dir.join("server.json")).ok()?;
    let info: Value = serde_json::from_str(&text).ok()?;
    if let Some(socket) = info.get("socket").and_then(Value::as_str) {
        return Some(Endpoint::Unix(PathBuf::from(socket)));
    }
    Some(Endpoint::Tcp {
        url: info.get("url")?.as_str()?.to_string(),
        token: info.get("token")?.as_str()?.to_string(),
    })
}

/// Send one request; returns the status and the whole body.
pub fn request(
    endpoint: &Endpoint,
    method: &str,
    target: &str,
    body: Option<&Value>,
    timeout: Duration,
) -> io::Result<(u16, Vec<u8>)> {
    let body = body
        .map(serde_json::to_vec)
        .transpose()
        .map_err(io::Error::other)?;
    let mut head =
        format!("{method} {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    if let Endpoint::Tcp { token, .. } = endpoint {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(body) = &body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    let mut raw = Vec::new();
    match endpoint {
        Endpoint::Tcp { url, .. } => {
            let addr = url.trim_start_matches("http://");
            let mut stream = TcpStream::connect(addr)?;
            stream.set_read_timeout(Some(timeout))?;
            exchange(&mut stream, &head, body.as_deref(), &mut raw)?;
        }
        Endpoint::Unix(path) => unix_exchange(path, timeout, &head, body.as_deref(), &mut raw)?,
    }
    http::parse_response(&raw)
}

fn exchange(
    stream: &mut (impl Read + Write),
    head: &str,
    body: Option<&[u8]>,
    raw: &mut Vec<u8>,
) -> io::Result<()> {
    stream.write_all(head.as_bytes())?;
    if let Some(body) = body {
        stream.write_all(body)?;
    }
    stream.flush()?;
    stream.read_to_end(raw).map(drop)
}

#[cfg(unix)]
fn unix_exchange(
    path: &Path,
    timeout: Duration,
    head: &str,
    body: Option<&[u8]>,
    raw: &mut Vec<u8>,
) -> io::Result<()> {
    let mut stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    exchange(&mut stream, head, body, raw)
}

#[cfg(not(unix))]
fn unix_exchange(
    _path: &Path,
    _timeout: Duration,
    _head: &str,
    _body: Option<&[u8]>,
    _raw: &mut Vec<u8>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Unix sockets need macOS or Linux",
    ))
}

/// JSON from a request, or the server's error message.
pub fn json(
    endpoint: &Endpoint,
    method: &str,
    target: &str,
    body: Option<&Value>,
) -> io::Result<Value> {
    let (status, bytes) = request(endpoint, method, target, body, Duration::from_secs(20))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if status == 200 {
        Ok(value)
    } else {
        let message = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("request failed");
        Err(io::Error::other(format!("{status}: {message}")))
    }
}

/// The server's status, failing fast when nothing answers.
pub fn status(endpoint: &Endpoint) -> io::Result<Value> {
    let (code, bytes) = request(endpoint, "GET", "/v1/status", None, Duration::from_secs(3))?;
    if code != 200 {
        return Err(io::Error::other(format!("status returned {code}")));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Ask the running window to open `path` and search `query`. Returns false
/// when no window answers, so the caller starts one.
pub fn forward_open(path: Option<&Path>, query: Option<&str>) -> bool {
    let Some(endpoint) = discover() else {
        return false;
    };
    if status(&endpoint).is_err() {
        return false;
    }
    let body = json!({ "path": path, "query": query });
    json(&endpoint, "POST", "/v1/open", Some(&body)).is_ok()
}
