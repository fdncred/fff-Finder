//! Just enough HTTP/1.1 for the control protocol: one request per
//! connection, `Content-Length` bodies, and responses that end when the
//! connection closes.

use std::io::{self, BufRead, BufReader, Read, Write};

const MAX_HEAD: usize = 64 * 1024;
pub const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub authorization: Option<String>,
    pub body: Vec<u8>,
}

impl Request {
    /// The first value of a query parameter.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every value of a repeatable query parameter.
    pub fn params(&self, name: &str) -> Vec<String> {
        self.query
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }
}

pub fn read_request(stream: impl Read) -> io::Result<Request> {
    let mut reader = BufReader::new(stream.take((MAX_HEAD + MAX_BODY) as u64));
    let mut line = String::new();
    let mut head = 0;
    let mut read_line = |reader: &mut BufReader<_>, line: &mut String| -> io::Result<()> {
        line.clear();
        let n = reader.read_line(line)?;
        head += n;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed",
            ));
        }
        if head > MAX_HEAD {
            return Err(invalid("request head too large"));
        }
        Ok(())
    };
    read_line(&mut reader, &mut line)?;
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().ok_or_else(|| invalid("no request target"))?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut request = Request {
        method,
        path: percent_decode(path, false),
        query: parse_query(query),
        ..Default::default()
    };
    let mut length = 0usize;
    loop {
        read_line(&mut reader, &mut line)?;
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                length = value.parse().map_err(|_| invalid("bad Content-Length"))?;
            }
            "authorization" => request.authorization = Some(value.to_string()),
            _ => {}
        }
    }
    if length > MAX_BODY {
        return Err(invalid("request body too large"));
    }
    request.body = vec![0; length];
    reader.read_exact(&mut request.body)?;
    Ok(request)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k, true), percent_decode(v, true))
        })
        .collect()
}

/// Decode `%XX` escapes, and `+` as a space in query strings.
pub fn percent_decode(text: &str, plus_is_space: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' if plus_is_space => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encode a query parameter value.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// Write the status line and headers. Without `length` the body ends when
/// the connection closes.
pub fn write_head(
    out: &mut impl Write,
    status: u16,
    content_type: &str,
    length: Option<usize>,
) -> io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nConnection: close\r\n",
        reason(status)
    )?;
    if let Some(length) = length {
        write!(out, "Content-Length: {length}\r\n")?;
    }
    out.write_all(b"\r\n")
}

pub fn write_json(out: &mut impl Write, status: u16, value: &serde_json::Value) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_head(out, status, "application/json", Some(body.len()))?;
    out.write_all(&body)?;
    out.flush()
}

pub fn write_error(out: &mut impl Write, status: u16, message: &str) -> io::Result<()> {
    write_json(out, status, &serde_json::json!({ "error": message }))
}

/// Stream a JSON array, one element at a time.
pub fn write_array<I>(out: &mut impl Write, items: I) -> io::Result<()>
where
    I: IntoIterator<Item = serde_json::Value>,
{
    write_head(out, 200, "application/json", None)?;
    let mut out = io::BufWriter::new(out);
    out.write_all(b"[")?;
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            out.write_all(b",")?;
        }
        serde_json::to_writer(&mut out, &item).map_err(io::Error::other)?;
    }
    out.write_all(b"]")?;
    out.flush()
}

/// Split a raw response into status and body.
pub fn parse_response(raw: &[u8]) -> io::Result<(u16, Vec<u8>)> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| invalid("no response head"))?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| invalid("bad response head"))?;
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| invalid("bad status line"))?;
    Ok((status, raw[split + 4..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_queries() {
        let q = parse_query("query=main+rs&pattern=a%26b&pattern=%E2%9C%93&flag");
        assert_eq!(
            q,
            [
                ("query".to_string(), "main rs".to_string()),
                ("pattern".into(), "a&b".into()),
                ("pattern".into(), "✓".into()),
                ("flag".into(), "".into()),
            ]
        );
        assert_eq!(percent_decode("100%", true), "100%");
        assert_eq!(percent_decode("%zz", true), "%zz");
        assert_eq!(percent_encode("a b&c/ü"), "a%20b%26c/%C3%BC");
    }

    #[test]
    fn reads_a_request() {
        let raw = b"POST /v1/show?x=1 HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\nAuthorization: Bearer t\r\n\r\n{}";
        let r = read_request(&raw[..]).unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/v1/show"));
        assert_eq!(r.param("x"), Some("1"));
        assert_eq!(r.authorization.as_deref(), Some("Bearer t"));
        assert_eq!(r.body, b"{}");
        assert!(
            read_request(&b"GET /x HTTP/1.1\r\nContent-Length: 99999999999\r\n\r\n"[..]).is_err()
        );
    }
}
