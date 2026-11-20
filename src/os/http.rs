#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use crate::os::Waker;
use crate::ui::types::Error;

/// HTTP request method supported by [`HttpClient`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    /// HTTP `GET` request.
    Get,
    /// HTTP `POST` request.
    Post,
    /// HTTP `PUT` request.
    Put,
    /// HTTP `PATCH` request.
    Patch,
    /// HTTP `DELETE` request.
    Delete,
    /// HTTP `HEAD` request.
    Head,
    /// HTTP `OPTIONS` request.
    Options,
}

impl HttpMethod {
    /// Returns the uppercase HTTP method token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

/// Hardened response, header, redirect, and timeout limits enforced by [`HttpClient`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpLimits {
    /// Maximum HTTP response header block size in bytes.
    pub max_header_bytes: usize,
    /// Maximum response body size in bytes.
    pub max_response_bytes: usize,
    /// Maximum number of HTTP `3xx` redirects followed.
    pub max_redirects: u8,
    /// Overall request timeout in milliseconds.
    pub timeout_ms: u64,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: 32 * 1024,
            max_response_bytes: 8 * 1024 * 1024,
            max_redirects: 5,
            timeout_ms: 30_000,
        }
    }
}

/// Outgoing HTTP/1.1 request descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    /// HTTP request method (`GET`, `POST`, etc.).
    pub method: HttpMethod,
    /// Target `http://` or `https://` URL.
    pub url: String,
    /// Outgoing `(name, value)` header pairs.
    pub headers: Vec<(String, String)>,
    /// Optional request payload bytes.
    pub body: Vec<u8>,
    /// Bounds and timeout configuration applied to this request.
    pub limits: HttpLimits,
}

impl HttpRequest {
    /// Creates an HTTP request with default [`HttpLimits`].
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
            limits: HttpLimits::default(),
        }
    }

    /// Creates a `GET` request.
    pub fn get(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Get, url)
    }

    /// Creates a `POST` request.
    pub fn post(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Post, url)
    }

    /// Creates a `PUT` request.
    pub fn put(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Put, url)
    }

    /// Creates a `PATCH` request.
    pub fn patch(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Patch, url)
    }

    /// Creates a `DELETE` request.
    pub fn delete(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Delete, url)
    }

    /// Appends a request header (`name: value`).
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Attaches raw request body bytes.
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Attaches a UTF-8 JSON payload and sets `Content-Type: application/json`.
    pub fn json(mut self, json: impl Into<String>) -> Self {
        let has_ct = self
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        if !has_ct {
            self.headers.push((
                String::from("Content-Type"),
                String::from("application/json"),
            ));
        }
        self.body = json.into().into_bytes();
        self
    }

    /// Sets the network timeout in milliseconds.
    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.limits.timeout_ms = timeout_ms;
        self
    }

    /// Overrides all [`HttpLimits`] for this request.
    pub fn limits(mut self, limits: HttpLimits) -> Self {
        self.limits = limits;
        self
    }
}

/// Completed HTTP response payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    /// Numeric HTTP status code (e.g. `200`, `404`).
    pub status: u16,
    /// Parsed response header `(name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// Bounded raw response body bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Returns `true` for HTTP `2xx` success status codes.
    pub const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// Performs a case-insensitive lookup of the first matching response header.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Decodes the response body as UTF-8 text.
    pub fn text(&self) -> Result<&str, Error> {
        core::str::from_utf8(&self.body)
            .map_err(|e| Error::InvalidInput(format!("HTTP response body is not valid UTF-8: {e}")))
    }
}

/// Non-blocking handle for an in-flight [`HttpClient`] request.
pub struct HttpHandle {
    result: Arc<Mutex<Option<Result<HttpResponse, Error>>>>,
    #[cfg(all(target_arch = "wasm32", feature = "network"))]
    _wasm_keepalive: Option<wasm_transport::WasmHttpKeepalive>,
}

impl HttpHandle {
    /// Polls for completion, returning `Some(result)` once the response or error arrives.
    pub fn poll(&mut self) -> Option<Result<HttpResponse, Error>> {
        self.result.lock().ok().and_then(|mut guard| guard.take())
    }

    /// Returns `true` if a response or error is ready to be polled.
    pub fn is_done(&self) -> bool {
        self.result.lock().ok().is_some_and(|guard| guard.is_some())
    }
}

/// Cross-platform sandboxed HTTP/1.1 + TLS (`native`) and XHR (`wasm32`) client.
pub struct HttpClient;

impl HttpClient {
    /// Spawns a non-blocking HTTP request that wakes the UI event loop via `waker`.
    pub fn send(req: HttpRequest, waker: Waker) -> HttpHandle {
        let result = Arc::new(Mutex::new(None));

        #[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
        {
            let slot = Arc::clone(&result);
            std::thread::spawn(move || {
                let outcome = native_transport::execute_request(&req);
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(outcome);
                }
                waker.wake();
            });
            HttpHandle { result }
        }

        #[cfg(all(target_arch = "wasm32", feature = "network"))]
        {
            let keepalive =
                match wasm_transport::start_request(&req, Arc::clone(&result), waker.clone()) {
                    Ok(k) => Some(k),
                    Err(e) => {
                        if let Ok(mut guard) = result.lock() {
                            *guard = Some(Err(e));
                        }
                        waker.wake();
                        None
                    }
                };
            HttpHandle {
                result,
                _wasm_keepalive: keepalive,
            }
        }

        #[cfg(not(feature = "network"))]
        {
            let _ = req;
            if let Ok(mut guard) = result.lock() {
                *guard = Some(Err(Error::Io(String::from(
                    "HTTP support requires the `network` feature",
                ))));
            }
            waker.wake();
            HttpHandle { result }
        }
    }

    /// Convenience non-blocking `GET` request.
    pub fn get(url: impl Into<String>, waker: Waker) -> HttpHandle {
        Self::send(HttpRequest::get(url), waker)
    }

    /// Convenience non-blocking `POST` request with raw body bytes.
    pub fn post(url: impl Into<String>, body: impl Into<Vec<u8>>, waker: Waker) -> HttpHandle {
        Self::send(HttpRequest::post(url).body(body), waker)
    }

    /// Executes an HTTP request synchronously on native targets (for CLI tools and tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn send_sync(req: &HttpRequest) -> Result<HttpResponse, Error> {
        #[cfg(feature = "network")]
        {
            native_transport::execute_request(req)
        }
        #[cfg(not(feature = "network"))]
        {
            let _ = req;
            Err(Error::Io(String::from(
                "HTTP support requires the `network` feature",
            )))
        }
    }
}

#[cfg(any(test, all(not(target_arch = "wasm32"), feature = "network")))]
pub(crate) mod http_wire {
    use std::io::{BufRead, Read};

    use super::{HttpLimits, HttpMethod, HttpRequest, HttpResponse};
    use crate::ui::types::Error;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ParsedHttpUrl {
        pub tls: bool,
        pub host: String,
        pub port: u16,
        pub path_and_query: String,
    }

    fn contains_ctl_or_crlf(s: &str) -> bool {
        s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
    }

    pub fn parse_http_url(url: &str) -> Result<ParsedHttpUrl, Error> {
        if contains_ctl_or_crlf(url) {
            return Err(Error::InvalidInput(String::from(
                "HTTP URL contains forbidden control characters",
            )));
        }
        let (tls, rest_with_frag) = if let Some(r) = url.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = url.strip_prefix("http://") {
            (false, r)
        } else {
            return Err(Error::InvalidInput(String::from(
                "HTTP URL must begin with http:// or https://",
            )));
        };

        let rest = rest_with_frag.split('#').next().unwrap_or(rest_with_frag);
        let split_idx = rest.find(['/', '?']);
        let (authority, path_and_query) = match split_idx {
            Some(idx) => {
                let tail = &rest[idx..];
                if tail.starts_with('?') {
                    (&rest[..idx], format!("/{tail}"))
                } else {
                    (&rest[..idx], tail.to_owned())
                }
            }
            None => (rest, String::from("/")),
        };

        if authority.is_empty() || authority.contains('@') {
            return Err(Error::InvalidInput(String::from(
                "HTTP URL authority is empty or contains unsupported userinfo",
            )));
        }

        let (host, port) = if let Some(after_open) = authority.strip_prefix('[') {
            let close = after_open.find(']').ok_or_else(|| {
                Error::InvalidInput(String::from("malformed IPv6 host in HTTP URL"))
            })?;
            let h = &after_open[..close];
            let rem = &after_open[close + 1..];
            let port = if let Some(p_str) = rem.strip_prefix(':') {
                p_str
                    .parse::<u16>()
                    .map_err(|_| Error::InvalidInput(String::from("invalid HTTP port")))?
            } else if rem.is_empty() {
                if tls {
                    443
                } else {
                    80
                }
            } else {
                return Err(Error::InvalidInput(String::from(
                    "invalid suffix after IPv6 host",
                )));
            };
            (h.to_owned(), port)
        } else if let Some((h, p_str)) = authority.rsplit_once(':') {
            let port = p_str
                .parse::<u16>()
                .map_err(|_| Error::InvalidInput(String::from("invalid HTTP port")))?;
            (h.to_owned(), port)
        } else {
            (authority.to_owned(), if tls { 443 } else { 80 })
        };

        if host.is_empty() {
            return Err(Error::InvalidInput(String::from("HTTP URL missing host")));
        }

        Ok(ParsedHttpUrl {
            tls,
            host,
            port,
            path_and_query,
        })
    }

    #[cfg(feature = "network")]
    pub fn resolve_redirect_url(base: &ParsedHttpUrl, location: &str) -> Result<String, Error> {
        let loc = location.trim();
        if contains_ctl_or_crlf(loc) || loc.is_empty() {
            return Err(Error::InvalidInput(String::from(
                "invalid redirect Location header",
            )));
        }
        if loc.starts_with("http://") || loc.starts_with("https://") {
            return Ok(loc.to_owned());
        }
        let scheme = if base.tls { "https" } else { "http" };
        let default_port = if base.tls { 443 } else { 80 };
        let host_fmt = if base.host.contains(':') {
            format!("[{}]", base.host)
        } else {
            base.host.clone()
        };
        let authority = if base.port == default_port {
            host_fmt
        } else {
            format!("{host_fmt}:{}", base.port)
        };
        if let Some(after_slashes) = loc.strip_prefix("//") {
            return Ok(format!("{scheme}://{after_slashes}"));
        }
        if loc.starts_with('/') {
            return Ok(format!("{scheme}://{authority}{loc}"));
        }
        let base_path = base.path_and_query.split('?').next().unwrap_or("/");
        let dir = match base_path.rfind('/') {
            Some(pos) => &base_path[..=pos],
            None => "/",
        };
        Ok(format!("{scheme}://{authority}{dir}{loc}"))
    }

    pub fn encode_request(req: &HttpRequest, parsed: &ParsedHttpUrl) -> Result<Vec<u8>, Error> {
        let default_port = if parsed.tls { 443 } else { 80 };
        let host_fmt = if parsed.host.contains(':') {
            format!("[{}]", parsed.host)
        } else {
            parsed.host.clone()
        };
        let host_header = if parsed.port == default_port {
            host_fmt
        } else {
            format!("{host_fmt}:{}", parsed.port)
        };

        let mut raw = String::new();
        raw.push_str(req.method.as_str());
        raw.push(' ');
        raw.push_str(&parsed.path_and_query);
        raw.push_str(" HTTP/1.1\r\n");

        let mut has_host = false;
        let mut has_ua = false;
        let mut has_conn = false;
        let mut has_cl = false;
        let mut has_ae = false;

        for (name, value) in &req.headers {
            if contains_ctl_or_crlf(name) || contains_ctl_or_crlf(value) || name.is_empty() {
                return Err(Error::InvalidInput(String::from(
                    "HTTP header contains invalid name or control characters",
                )));
            }
            if name.contains(':') || name.bytes().any(|b| b <= 0x20 || b >= 0x7F) {
                return Err(Error::InvalidInput(String::from(
                    "HTTP header name contains invalid token bytes",
                )));
            }
            if name.eq_ignore_ascii_case("host") {
                has_host = true;
            } else if name.eq_ignore_ascii_case("user-agent") {
                has_ua = true;
            } else if name.eq_ignore_ascii_case("connection") {
                has_conn = true;
            } else if name.eq_ignore_ascii_case("content-length") {
                has_cl = true;
            } else if name.eq_ignore_ascii_case("accept-encoding") {
                has_ae = true;
            }
            raw.push_str(name);
            raw.push_str(": ");
            raw.push_str(value);
            raw.push_str("\r\n");
        }

        if !has_host {
            raw.push_str("Host: ");
            raw.push_str(&host_header);
            raw.push_str("\r\n");
        }
        if !has_ua {
            raw.push_str("User-Agent: lumenic/0.1\r\n");
        }
        if !has_ae {
            raw.push_str("Accept-Encoding: identity\r\n");
        }
        if !has_conn {
            raw.push_str("Connection: close\r\n");
        }
        let needs_cl = !req.body.is_empty()
            || matches!(
                req.method,
                HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch
            );
        if !has_cl && needs_cl {
            raw.push_str(&format!("Content-Length: {}\r\n", req.body.len()));
        }
        raw.push_str("\r\n");

        let total = raw
            .len()
            .checked_add(req.body.len())
            .ok_or_else(|| Error::InvalidInput(String::from("HTTP request size overflow")))?;
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(raw.as_bytes());
        out.extend_from_slice(&req.body);
        Ok(out)
    }

    fn read_line_bounded<R: BufRead>(
        reader: &mut R,
        total_header_bytes: &mut usize,
        max_header_bytes: usize,
    ) -> Result<Option<String>, Error> {
        let mut line = Vec::new();
        loop {
            let avail = reader
                .fill_buf()
                .map_err(|e| Error::Io(format!("HTTP read failed: {e}")))?;
            if avail.is_empty() {
                if line.is_empty() {
                    return Ok(None);
                }
                break;
            }
            if let Some(pos) = avail.iter().position(|&b| b == b'\n') {
                let take = pos + 1;
                *total_header_bytes = total_header_bytes.checked_add(take).ok_or_else(|| {
                    Error::InvalidInput(String::from("HTTP header byte count overflow"))
                })?;
                if *total_header_bytes > max_header_bytes {
                    return Err(Error::InvalidInput(String::from(
                        "HTTP response headers exceeded max_header_bytes",
                    )));
                }
                line.extend_from_slice(&avail[..take]);
                reader.consume(take);
                break;
            }
            let take = avail.len();
            *total_header_bytes = total_header_bytes.checked_add(take).ok_or_else(|| {
                Error::InvalidInput(String::from("HTTP header byte count overflow"))
            })?;
            if *total_header_bytes > max_header_bytes {
                return Err(Error::InvalidInput(String::from(
                    "HTTP response headers exceeded max_header_bytes",
                )));
            }
            line.extend_from_slice(avail);
            reader.consume(take);
        }
        Ok(Some(
            String::from_utf8_lossy(&line)
                .trim_end_matches(['\r', '\n'])
                .to_owned(),
        ))
    }

    fn read_exact_bounded<R: Read>(
        reader: &mut R,
        len: usize,
        max_bytes: usize,
    ) -> Result<Vec<u8>, Error> {
        if len > max_bytes {
            return Err(Error::InvalidInput(String::from(
                "HTTP response body exceeds max_response_bytes",
            )));
        }
        let mut buf = vec![0u8; len];
        let mut read = 0usize;
        while read < len {
            match reader.read(&mut buf[read..]) {
                Ok(0) => return Err(Error::Io(String::from("HTTP response body truncated"))),
                Ok(n) => read += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::Io(format!("HTTP body read failed: {e}"))),
            }
        }
        Ok(buf)
    }

    fn read_chunked_body<R: BufRead>(reader: &mut R, max_bytes: usize) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        let mut header_budget = 0usize;
        loop {
            let line = read_line_bounded(reader, &mut header_budget, 16 * 1024)?
                .ok_or_else(|| Error::Io(String::from("unexpected EOF in chunked HTTP body")))?;
            let hex_part = line.split(';').next().unwrap_or("").trim();
            let chunk_len = usize::from_str_radix(hex_part, 16)
                .map_err(|_| Error::Io(format!("invalid HTTP chunk size: {line}")))?;
            if chunk_len == 0 {
                // Consume optional trailers until empty line (ignore EOF if server closed cleanly).
                while let Some(trailer) = read_line_bounded(reader, &mut header_budget, 16 * 1024)?
                {
                    if trailer.is_empty() {
                        break;
                    }
                }
                break;
            }
            let next_len = out.len().checked_add(chunk_len).ok_or_else(|| {
                Error::InvalidInput(String::from("chunked HTTP body size overflow"))
            })?;
            if next_len > max_bytes {
                return Err(Error::InvalidInput(String::from(
                    "chunked HTTP body exceeds max_response_bytes",
                )));
            }
            let chunk = read_exact_bounded(reader, chunk_len, max_bytes)?;
            out.extend_from_slice(&chunk);
            let mut crlf = [0u8; 2];
            let mut got = 0usize;
            while got < 2 {
                match reader.read(&mut crlf[got..]) {
                    Ok(0) => return Err(Error::Io(String::from("truncated HTTP chunk CRLF"))),
                    Ok(n) => got += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(Error::Io(format!("HTTP chunk CRLF read failed: {e}"))),
                }
            }
        }
        Ok(out)
    }

    fn read_to_end_bounded<R: Read>(reader: &mut R, max_bytes: usize) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        let mut tmp = [0u8; 8192];
        loop {
            match reader.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => {
                    let next_len = out.len().checked_add(n).ok_or_else(|| {
                        Error::InvalidInput(String::from("HTTP response size overflow"))
                    })?;
                    if next_len > max_bytes {
                        return Err(Error::InvalidInput(String::from(
                            "HTTP response body exceeds max_response_bytes",
                        )));
                    }
                    out.extend_from_slice(&tmp[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::Io(format!("HTTP body read failed: {e}"))),
            }
        }
        Ok(out)
    }

    pub fn read_response<R: BufRead>(
        reader: &mut R,
        method: HttpMethod,
        limits: HttpLimits,
    ) -> Result<HttpResponse, Error> {
        let mut header_bytes = 0usize;
        let status_line = read_line_bounded(reader, &mut header_bytes, limits.max_header_bytes)?
            .ok_or_else(|| Error::Io(String::from("HTTP empty response from server")))?;

        let mut parts = status_line.split_whitespace();
        let version = parts.next().unwrap_or("");
        if !version.starts_with("HTTP/1.") {
            return Err(Error::Io(format!(
                "unsupported HTTP status line: {status_line}"
            )));
        }
        let status: u16 = parts
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or_else(|| Error::Io(format!("invalid HTTP status code: {status_line}")))?;

        let mut headers = Vec::new();
        let mut content_length: Option<usize> = None;
        let mut is_chunked = false;

        while let Some(line) =
            read_line_bounded(reader, &mut header_bytes, limits.max_header_bytes)?
        {
            if line.is_empty() {
                break;
            }
            if let Some((name, val)) = line.split_once(':') {
                let k = name.trim().to_owned();
                let v = val.trim().to_owned();
                if k.eq_ignore_ascii_case("content-length") {
                    content_length = v.parse::<usize>().ok();
                } else if k.eq_ignore_ascii_case("transfer-encoding")
                    && v.to_ascii_lowercase().contains("chunked")
                {
                    is_chunked = true;
                }
                headers.push((k, v));
            }
        }

        let no_body = method == HttpMethod::Head
            || (100..200).contains(&status)
            || status == 204
            || status == 304;
        let body = if no_body {
            Vec::new()
        } else if is_chunked {
            read_chunked_body(reader, limits.max_response_bytes)?
        } else if let Some(len) = content_length {
            read_exact_bounded(reader, len, limits.max_response_bytes)?
        } else {
            read_to_end_bounded(reader, limits.max_response_bytes)?
        };

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
mod native_transport {
    use std::io::{self, BufReader, Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::sync::Arc;
    use std::time::Duration;

    use super::http_wire::{encode_request, parse_http_url, read_response, resolve_redirect_url};
    use super::{HttpMethod, HttpRequest, HttpResponse};
    use crate::ui::types::Error;

    enum NativeStream {
        Plain(TcpStream),
        Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
    }

    impl Read for NativeStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self {
                Self::Plain(s) => s.read(buf),
                Self::Tls(s) => s.read(buf),
            }
        }
    }

    impl Write for NativeStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            match self {
                Self::Plain(s) => s.write(buf),
                Self::Tls(s) => s.write(buf),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            match self {
                Self::Plain(s) => s.flush(),
                Self::Tls(s) => s.flush(),
            }
        }
    }

    fn connect_stream(
        host: &str,
        port: u16,
        tls: bool,
        timeout_ms: u64,
    ) -> Result<NativeStream, Error> {
        let timeout = Duration::from_millis(timeout_ms.max(100));
        let mut last_err = None;
        let addrs = (host, port)
            .to_socket_addrs()
            .map_err(|e| Error::Io(format!("HTTP DNS lookup failed for {host}:{port}: {e}")))?;
        let mut connected = None;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(s) => {
                    connected = Some(s);
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let tcp = connected.ok_or_else(|| {
            Error::Io(format!(
                "HTTP connect failed for {host}:{port}: {}",
                last_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| String::from("no resolved address"))
            ))
        })?;
        let _ = tcp.set_read_timeout(Some(timeout));
        let _ = tcp.set_write_timeout(Some(timeout));

        if !tls {
            return Ok(NativeStream::Plain(tcp));
        }

        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let server_name = rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| Error::InvalidInput(format!("invalid TLS server name: {host}")))?;
        let conn = rustls::ClientConnection::new(Arc::new(cfg), server_name)
            .map_err(|e| Error::Io(format!("TLS init failed: {e}")))?;
        Ok(NativeStream::Tls(Box::new(rustls::StreamOwned::new(
            conn, tcp,
        ))))
    }

    pub fn execute_request(initial: &HttpRequest) -> Result<HttpResponse, Error> {
        let mut current = initial.clone();
        let max_redirects = initial.limits.max_redirects;

        for hop in 0..=max_redirects {
            let parsed = parse_http_url(&current.url)?;
            let wire = encode_request(&current, &parsed)?;
            let mut stream = connect_stream(
                &parsed.host,
                parsed.port,
                parsed.tls,
                current.limits.timeout_ms,
            )?;
            stream
                .write_all(&wire)
                .and_then(|_| stream.flush())
                .map_err(|e| Error::Io(format!("HTTP write failed: {e}")))?;

            let mut reader = BufReader::new(stream);
            let resp = read_response(&mut reader, current.method, current.limits)?;

            let is_redirect = matches!(resp.status, 301 | 302 | 303 | 307 | 308);
            if is_redirect && hop < max_redirects {
                if let Some(loc) = resp.header("location") {
                    let next_url = resolve_redirect_url(&parsed, loc)?;
                    let next_parsed = parse_http_url(&next_url)?;
                    if next_parsed.host != parsed.host || next_parsed.port != parsed.port {
                        current.headers.retain(|(k, _)| {
                            !k.eq_ignore_ascii_case("authorization")
                                && !k.eq_ignore_ascii_case("cookie")
                        });
                    }
                    if matches!(resp.status, 301..=303)
                        && !matches!(current.method, HttpMethod::Get | HttpMethod::Head)
                    {
                        current.method = HttpMethod::Get;
                        current.body.clear();
                        current
                            .headers
                            .retain(|(k, _)| !k.eq_ignore_ascii_case("content-length"));
                    }
                    current.url = next_url;
                    continue;
                }
            }
            return Ok(resp);
        }

        Err(Error::Io(String::from("HTTP redirect limit exceeded")))
    }
}

#[cfg(all(target_arch = "wasm32", feature = "network"))]
mod wasm_transport {
    use std::sync::{Arc, Mutex};

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use web_sys::{Event, XmlHttpRequest};

    use super::{HttpRequest, HttpResponse};
    use crate::os::Waker;
    use crate::ui::types::Error;

    pub struct WasmHttpKeepalive {
        _xhr: XmlHttpRequest,
        _on_load: Closure<dyn FnMut(Event)>,
        _on_err: Closure<dyn FnMut(Event)>,
    }

    pub fn start_request(
        req: &HttpRequest,
        slot: Arc<Mutex<Option<Result<HttpResponse, Error>>>>,
        waker: Waker,
    ) -> Result<WasmHttpKeepalive, Error> {
        let xhr = XmlHttpRequest::new()
            .map_err(|_| Error::Io(String::from("XmlHttpRequest::new failed")))?;
        xhr.open(req.method.as_str(), &req.url)
            .map_err(|_| Error::Io(String::from("XmlHttpRequest::open failed")))?;
        let timeout_u32 = u32::try_from(req.limits.timeout_ms).unwrap_or(u32::MAX);
        xhr.set_timeout(timeout_u32);

        for (name, value) in &req.headers {
            xhr.set_request_header(name, value)
                .map_err(|_| Error::InvalidInput(format!("invalid XHR header: {name}")))?;
        }

        let xhr_load = xhr.clone();
        let slot_load = Arc::clone(&slot);
        let waker_load = waker.clone();
        let max_bytes = req.limits.max_response_bytes;
        let on_load = Closure::wrap(Box::new(move |_ev: Event| {
            let outcome = (|| -> Result<HttpResponse, Error> {
                let status = xhr_load
                    .status()
                    .map_err(|_| Error::Io(String::from("failed to read XHR status")))?;
                let raw_headers = xhr_load.get_all_response_headers().unwrap_or_default();
                let mut headers = Vec::new();
                for line in raw_headers.lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        headers.push((k.trim().to_owned(), v.trim().to_owned()));
                    }
                }
                let text = xhr_load
                    .response_text()
                    .map_err(|_| Error::Io(String::from("failed to read XHR response text")))?
                    .unwrap_or_default();
                let body = text.into_bytes();
                if body.len() > max_bytes {
                    return Err(Error::InvalidInput(String::from(
                        "HTTP response exceeds max_response_bytes",
                    )));
                }
                Ok(HttpResponse {
                    status,
                    headers,
                    body,
                })
            })();
            if let Ok(mut guard) = slot_load.lock() {
                *guard = Some(outcome);
            }
            waker_load.wake();
        }) as Box<dyn FnMut(Event)>);

        let slot_err = Arc::clone(&slot);
        let waker_err = waker;
        let on_err = Closure::wrap(Box::new(move |_ev: Event| {
            if let Ok(mut guard) = slot_err.lock() {
                *guard = Some(Err(Error::Io(String::from("browser HTTP request failed"))));
            }
            waker_err.wake();
        }) as Box<dyn FnMut(Event)>);

        xhr.set_onload(Some(on_load.as_ref().unchecked_ref()));
        xhr.set_onerror(Some(on_err.as_ref().unchecked_ref()));
        xhr.set_ontimeout(Some(on_err.as_ref().unchecked_ref()));

        if req.body.is_empty() {
            xhr.send()
                .map_err(|_| Error::Io(String::from("XmlHttpRequest::send failed")))?;
        } else if let Ok(text) = core::str::from_utf8(&req.body) {
            xhr.send_with_opt_str(Some(text))
                .map_err(|_| Error::Io(String::from("XmlHttpRequest::send_with_opt_str failed")))?;
        } else {
            xhr.send_with_opt_u8_array(Some(&req.body)).map_err(|_| {
                Error::Io(String::from(
                    "XmlHttpRequest::send_with_opt_u8_array failed",
                ))
            })?;
        }

        Ok(WasmHttpKeepalive {
            _xhr: xhr,
            _on_load: on_load,
            _on_err: on_err,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    #[cfg(feature = "network")]
    use std::io::{Read, Write};
    #[cfg(feature = "network")]
    use std::net::TcpListener;
    #[cfg(feature = "network")]
    use std::sync::atomic::{AtomicBool, Ordering};
    #[cfg(feature = "network")]
    use std::sync::Arc;
    #[cfg(feature = "network")]
    use std::thread;
    #[cfg(feature = "network")]
    use std::time::{Duration, Instant};

    use super::http_wire::{encode_request, parse_http_url, read_response};
    #[cfg(feature = "network")]
    use super::HttpClient;
    use super::{HttpLimits, HttpMethod, HttpRequest};
    #[cfg(feature = "network")]
    use crate::os::Waker;

    #[test]
    fn url_parser_and_header_injection_rejected() {
        let p = parse_http_url("https://api.example.com:8443/v1/items?q=1#frag").expect("valid");
        assert!(p.tls);
        assert_eq!(p.host, "api.example.com");
        assert_eq!(p.port, 8443);
        assert_eq!(p.path_and_query, "/v1/items?q=1");

        let p6 = parse_http_url("http://[::1]:8080/health").expect("valid ipv6");
        assert!(!p6.tls);
        assert_eq!(p6.host, "::1");
        assert_eq!(p6.port, 8080);
        assert_eq!(p6.path_and_query, "/health");

        assert!(parse_http_url("ftp://example.com/a").is_err());
        assert!(parse_http_url("http://example.com/\r\nInjected: 1").is_err());

        let bad_req = HttpRequest::get("http://example.com/").header("X-Bad\r\nEvil", "1");
        assert!(encode_request(&bad_req, &parse_http_url(&bad_req.url).unwrap()).is_err());
    }

    #[test]
    fn chunked_and_bounded_response_parsing() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nX-Test: yes\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let mut cursor = BufReader::new(&raw[..]);
        let resp = read_response(&mut cursor, HttpMethod::Get, HttpLimits::default()).expect("ok");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.header("x-test"), Some("yes"));
        assert_eq!(resp.text().unwrap(), "hello world");

        let too_big = b"HTTP/1.1 200 OK\r\nContent-Length: 9999\r\n\r\nabc";
        let mut cursor2 = BufReader::new(&too_big[..]);
        let tight = HttpLimits {
            max_response_bytes: 64,
            ..HttpLimits::default()
        };
        assert!(read_response(&mut cursor2, HttpMethod::Get, tight).is_err());
    }

    #[cfg(feature = "network")]
    #[test]
    fn loopback_http_get_post_and_redirect_with_waker() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut req_buf = [0u8; 2048];
                let n = stream.read(&mut req_buf).expect("read");
                let req_str = String::from_utf8_lossy(&req_buf[..n]);
                if req_str.starts_with("GET /redirect ") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\n\r\n",
                    );
                } else if req_str.starts_with("GET /final ") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n",
                    );
                } else if req_str.starts_with("POST /echo ") {
                    let body = req_str.split("\r\n\r\n").nth(1).unwrap_or("");
                    let reply = format!(
                        "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(reply.as_bytes());
                }
            }
        });

        let woken = Arc::new(AtomicBool::new(false));
        let woken_flag = Arc::clone(&woken);
        let waker = Waker::from_fn(move || woken_flag.store(true, Ordering::SeqCst));

        let mut handle = HttpClient::get(format!("http://127.0.0.1:{port}/redirect"), waker);
        let deadline = Instant::now() + Duration::from_secs(3);
        let resp = loop {
            if let Some(res) = handle.poll() {
                break res.expect("redirect response");
            }
            assert!(Instant::now() < deadline, "HTTP handle timed out");
            thread::sleep(Duration::from_millis(5));
        };
        assert!(woken.load(Ordering::SeqCst));
        assert_eq!(resp.status, 200);
        assert_eq!(resp.text().unwrap(), "ok");

        let post_req =
            HttpRequest::post(format!("http://127.0.0.1:{port}/echo")).json(r#"{"hi":1}"#);
        let post_resp = HttpClient::send_sync(&post_req).expect("post response");
        assert_eq!(post_resp.status, 201);
        assert_eq!(post_resp.text().unwrap(), r#"{"hi":1}"#);

        let _ = server.join();
    }
}
