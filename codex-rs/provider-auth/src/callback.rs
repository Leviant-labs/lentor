//! Loopback HTTP callback handling for CommandCode OAuth.

use crate::ApiKey;
use crate::AuthError;
use serde::Deserialize;
use std::collections::HashMap;
use std::io;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use url::Url;

/// Preferred fixed port for the loopback callback listener.
const CALLBACK_PORT: u16 = 5959;
/// Studio origin allowed to post browser callbacks to the loopback listener.
const STUDIO_ORIGIN: &str = "https://commandcode.ai";
const MAX_HEADER_BYTES: usize = 8 * 1024;
pub(crate) const MAX_BODY_BYTES: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(25);

pub(crate) struct LoopbackListeners {
    ipv4: TcpListener,
    ipv6: Option<TcpListener>,
    pub(crate) port: u16,
}

/// Binds the fixed callback port when available, then falls back to an
/// ephemeral port while keeping the same port for IPv4 and IPv6 when possible.
pub(crate) async fn bind_loopback() -> io::Result<LoopbackListeners> {
    let ipv4 = match TcpListener::bind(("127.0.0.1", CALLBACK_PORT)).await {
        Ok(listener) => listener,
        Err(_) => TcpListener::bind(("127.0.0.1", 0)).await?,
    };
    let port = ipv4.local_addr()?.port();
    let ipv6 = TcpListener::bind(("::1", port)).await.ok();
    Ok(LoopbackListeners { ipv4, ipv6, port })
}

/// Serves callback requests until a valid callback arrives or the task is
/// aborted. Invalid requests are answered and rejected without stopping the
/// listener, so a state mismatch does not end the login attempt.
pub(crate) async fn serve_callback(
    listeners: LoopbackListeners,
    state: String,
    tx: oneshot::Sender<ApiKey>,
) {
    let LoopbackListeners { ipv4, ipv6, .. } = listeners;
    loop {
        let accepted = match ipv6.as_ref() {
            Some(ipv6) => tokio::select! {
                result = ipv4.accept() => result,
                result = ipv6.accept() => result,
            },
            None => ipv4.accept().await,
        };
        let (mut stream, _) = match accepted {
            Ok(connection) => connection,
            Err(_) => {
                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                continue;
            }
        };
        match tokio::time::timeout(REQUEST_TIMEOUT, handle_connection(&mut stream, &state)).await {
            Ok(Some(key)) => {
                let _ = tx.send(key);
                return;
            }
            Ok(None) => {}
            Err(_) => respond(&mut stream, Status::RequestTimeout).await,
        }
    }
}

async fn handle_connection(stream: &mut TcpStream, state: &str) -> Option<ApiKey> {
    let request = match read_request(stream).await {
        Ok(request) => request,
        Err(status) => {
            respond(stream, status).await;
            return None;
        }
    };
    if request.method == "OPTIONS" {
        respond(stream, Status::NoContent).await;
        return None;
    }
    match request.into_callback(state) {
        Ok(key) => {
            respond(stream, Status::Ok).await;
            Some(key)
        }
        Err(status) => {
            respond(stream, status).await;
            None
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Request {
    fn into_callback(self, state: &str) -> Result<ApiKey, Status> {
        if self.method != "POST" {
            return Err(Status::MethodNotAllowed);
        }
        if self.path != "/callback" {
            return Err(Status::NotFound);
        }
        let content_type = self.headers.get("content-type").map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or(value)
                .trim()
                .to_ascii_lowercase()
        });
        let callback = match content_type.as_deref() {
            Some("application/json") => {
                serde_json::from_slice(&self.body).map_err(|_| Status::BadRequest)?
            }
            Some("application/x-www-form-urlencoded") => parse_form(&self.body)?,
            _ => return Err(Status::UnsupportedMediaType),
        };
        if callback.state != state {
            return Err(Status::Forbidden);
        }
        let api_key = callback.api_key.trim();
        if api_key.is_empty() {
            return Err(Status::BadRequest);
        }
        Ok(ApiKey::new(api_key))
    }
}

async fn read_request(stream: &mut TcpStream) -> Result<Request, Status> {
    let mut buffer = Vec::with_capacity(1024);
    let header_end = loop {
        let found = find_header_end(&buffer);
        if found.is_some_and(|position| position > MAX_HEADER_BYTES) {
            return Err(Status::RequestHeaderFieldsTooLarge);
        }
        if let Some(position) = found {
            break position;
        }
        if buffer.len() >= MAX_HEADER_BYTES {
            return Err(Status::RequestHeaderFieldsTooLarge);
        }
        let mut chunk = [0u8; 1024];
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| Status::BadRequest)?;
        if read == 0 {
            return Err(Status::BadRequest);
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let header = std::str::from_utf8(&buffer[..header_end]).map_err(|_| Status::BadRequest)?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().ok_or(Status::BadRequest)?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().ok_or(Status::BadRequest)?.to_string();
    let target = request_parts.next().ok_or(Status::BadRequest)?;
    let version = request_parts.next().ok_or(Status::BadRequest)?;
    if !version.starts_with("HTTP/1.") || request_parts.next().is_some() {
        return Err(Status::BadRequest);
    }
    let path = target.split('?').next().unwrap_or(target).to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(Status::BadRequest)?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err(Status::BadRequest);
        }
        if headers.insert(name, value.trim().to_string()).is_some() {
            return Err(Status::BadRequest);
        }
    }

    if method == "POST" && headers.contains_key("transfer-encoding") {
        return Err(Status::LengthRequired);
    }
    let content_length = if method == "POST" {
        headers
            .get("content-length")
            .ok_or(Status::LengthRequired)?
            .parse::<usize>()
            .map_err(|_| Status::BadRequest)?
    } else {
        0
    };
    if content_length > MAX_BODY_BYTES {
        return Err(Status::PayloadTooLarge);
    }

    let body_start = header_end + 4;
    let mut body = buffer[body_start..].to_vec();
    if body.len() > content_length {
        return Err(Status::BadRequest);
    }
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let mut chunk = vec![0u8; remaining.min(4096)];
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| Status::BadRequest)?;
        if read == 0 {
            return Err(Status::BadRequest);
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Callback {
    #[serde(alias = "api_key")]
    api_key: String,
    state: String,
}

fn parse_form(body: &[u8]) -> Result<Callback, Status> {
    let mut api_key = None;
    let mut state = None;
    for (name, value) in url::form_urlencoded::parse(body) {
        match name.as_ref() {
            "apiKey" | "api_key" | "code" | "key" => api_key = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            _ => {}
        }
    }
    Ok(Callback {
        api_key: api_key.ok_or(Status::BadRequest)?,
        state: state.ok_or(Status::BadRequest)?,
    })
}

/// Parses a pasted callback payload, callback URL, or bare API key. A bare key
/// has no state to check; payloads and URLs must carry the expected state.
pub(crate) fn parse_manual(input: &str, state: &str) -> Result<ApiKey, AuthError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(AuthError::InvalidCallback);
    }
    if input.starts_with('{') {
        let callback: Callback =
            serde_json::from_str(input).map_err(|_| AuthError::InvalidCallback)?;
        if callback.state != state {
            return Err(AuthError::StateMismatch);
        }
        let api_key = callback.api_key.trim();
        if api_key.is_empty() {
            return Err(AuthError::InvalidCallback);
        }
        return Ok(ApiKey::new(api_key));
    }
    if let Ok(url) = Url::parse(input) {
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        match query.get("state") {
            Some(value) if value == state => {}
            Some(_) => return Err(AuthError::StateMismatch),
            None => return Err(AuthError::InvalidCallback),
        }
        let api_key = query
            .get("apiKey")
            .or_else(|| query.get("api_key"))
            .or_else(|| query.get("code"))
            .ok_or(AuthError::InvalidCallback)?
            .trim();
        if api_key.is_empty() {
            return Err(AuthError::InvalidCallback);
        }
        return Ok(ApiKey::new(api_key));
    }
    Ok(ApiKey::new(input))
}

#[derive(Clone, Copy)]
enum Status {
    NoContent,
    Ok,
    BadRequest,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    LengthRequired,
    PayloadTooLarge,
    UnsupportedMediaType,
    RequestHeaderFieldsTooLarge,
    RequestTimeout,
}

impl Status {
    fn line(self) -> &'static str {
        match self {
            Self::NoContent => "204 No Content",
            Self::Ok => "200 OK",
            Self::BadRequest => "400 Bad Request",
            Self::Forbidden => "403 Forbidden",
            Self::NotFound => "404 Not Found",
            Self::MethodNotAllowed => "405 Method Not Allowed",
            Self::RequestTimeout => "408 Request Timeout",
            Self::LengthRequired => "411 Length Required",
            Self::PayloadTooLarge => "413 Payload Too Large",
            Self::UnsupportedMediaType => "415 Unsupported Media Type",
            Self::RequestHeaderFieldsTooLarge => "431 Request Header Fields Too Large",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::NoContent => "",
            Self::Ok => "CommandCode callback received. Finish signing in from Codex.",
            Self::BadRequest => "Invalid callback request.",
            Self::Forbidden => "Callback state did not match.",
            Self::NotFound => "Not found.",
            Self::MethodNotAllowed => "Only POST is supported.",
            Self::RequestTimeout => "Request timed out.",
            Self::LengthRequired => "Content-Length is required.",
            Self::PayloadTooLarge => "Callback payload is too large.",
            Self::UnsupportedMediaType => "Unsupported content type.",
            Self::RequestHeaderFieldsTooLarge => "Request headers are too large.",
        }
    }
}

async fn respond(stream: &mut TcpStream, status: Status) {
    let message = status.message();
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: {STUDIO_ORIGIN}\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n{}",
        status.line(),
        message.len(),
        message
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}
