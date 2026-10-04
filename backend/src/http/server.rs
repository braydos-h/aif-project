//! Threaded HTTP/1.1 connection handling on `std::net`.
//!
//! One thread per connection, bounded by [`MAX_CONCURRENT_CONNECTIONS`];
//! one request per connection (`Connection: close`). Reads the request
//! line, headers, and body, then hands off to [`super::dispatch`] for
//! routing. Connections arriving above the cap get an immediate `503
//! server_busy` JSON response instead of spawning another thread's work.

use std::io::{BufRead, BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::request_id::new_request_id;
use super::response::{
    error_json, write_response, Response, CODE_BAD_REQUEST, CODE_INVALID_JSON, CODE_SERVER_BUSY,
};
use super::validation::MAX_BODY_BYTES;
use super::{dispatch, ServerState};

/// Max simultaneous connections handled. Excess connections are refused
/// with `503 server_busy` so one burst cannot exhaust threads/memory.
/// 64 comfortably covers the WebUI (sequential batch + demo + tape) while
/// bounding worst-case thread use on small field machines.
pub const MAX_CONCURRENT_CONNECTIONS: usize = 64;
/// Max request-line bytes (method + path + version).
pub const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
/// Max header lines per request (excluding the request line).
pub const MAX_HEADER_COUNT: usize = 100;
/// Max total header bytes per request (names + values).
pub const MAX_HEADER_BYTES: usize = 32 * 1024;

/// RAII guard: increments the active count on entry, decrements on drop so
/// every early return still releases its slot.
struct ActiveGuard<'a> {
    metrics: &'a crate::http::ServerMetrics,
}

impl<'a> ActiveGuard<'a> {
    fn enter(metrics: &'a crate::http::ServerMetrics) -> Option<Self> {
        let prev = metrics.active_connections.fetch_add(1, Ordering::SeqCst);
        if prev >= MAX_CONCURRENT_CONNECTIONS {
            metrics.active_connections.fetch_sub(1, Ordering::SeqCst);
            metrics.record_rejected();
            return None;
        }
        Some(ActiveGuard { metrics })
    }
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.metrics
            .active_connections
            .fetch_sub(1, Ordering::SeqCst);
    }
}

/// Serve on `host:port` (port 0 picks an ephemeral port, printed on stdout
/// so the launcher can read it). Blocks forever.
pub fn serve(state: Arc<ServerState>, host: &str, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind((host, port))?;
    let actual = listener.local_addr()?;
    println!("listening on http://{}:{}/", actual.ip(), actual.port());
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                // Best-effort burst shedding before spawning a thread: the
                // authoritative cap still runs inside `handle_connection`
                // (which closes most of the check-then-spawn race), but
                // refusing obvious overload here avoids a thread per excess
                // socket during a burst.
                if state.metrics.active() >= MAX_CONCURRENT_CONNECTIONS {
                    state.metrics.record_rejected();
                    state.metrics.record_request();
                    let request_id = new_request_id();
                    let response = Response::json(
                        503,
                        error_json(
                            CODE_SERVER_BUSY,
                            "Server is busy; try again shortly",
                            &request_id,
                        ),
                    );
                    let _ = write_response(&mut stream, &request_id, &response);
                    continue;
                }
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let _ = handle_connection(stream, &state);
                });
            }
            Err(e) => eprintln!("accept error: {}", e),
        }
    }
    Ok(())
}

/// Parsed request head: method, path (query stripped), and body length.
#[derive(Debug)]
struct RequestHead {
    method: String,
    path: String,
    content_length: usize,
    has_chunked_body: bool,
}

/// Client-error constructor: `ErrorKind::InvalidData` marks failures the
/// server answers with `400` instead of dropping the connection.
fn invalid_data(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

fn read_request_head(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<RequestHead>> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None); // client closed before sending anything
    }
    if request_line.len() > MAX_REQUEST_LINE_BYTES {
        return Err(invalid_data("Request line too large"));
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();

    let mut content_length: Option<usize> = None;
    let mut has_chunked_body = false;
    let mut header_count = 0usize;
    let mut header_bytes = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        header_count += 1;
        header_bytes += line.len();
        if header_count > MAX_HEADER_COUNT || header_bytes > MAX_HEADER_BYTES {
            return Err(invalid_data("Request headers too large"));
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                if content_length.is_some() {
                    return Err(invalid_data("Duplicate Content-Length"));
                }
                content_length = Some(
                    value
                        .trim()
                        .parse()
                        .map_err(|_| invalid_data("Invalid Content-Length"))?,
                );
            } else if name.eq_ignore_ascii_case("Transfer-Encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                has_chunked_body = true;
            }
        }
    }
    Ok(Some(RequestHead {
        method,
        path,
        content_length: content_length.unwrap_or(0),
        has_chunked_body,
    }))
}

/// Read the request line + headers + body from the connection and produce a
/// response. One connection = one request (Connection: close).
fn handle_connection(mut stream: TcpStream, state: &ServerState) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let request_id = new_request_id();
    // Bound concurrency before doing any I/O-bound work.
    let Some(_guard) = ActiveGuard::enter(&state.metrics) else {
        state.metrics.record_request();
        let response = Response::json(
            503,
            error_json(
                CODE_SERVER_BUSY,
                "Server is busy; try again shortly",
                &request_id,
            ),
        );
        return write_response(&mut stream, &request_id, &response);
    };
    let mut reader = BufReader::new(stream.try_clone()?);

    let head = match read_request_head(&mut reader) {
        Ok(Some(head)) => head,
        Ok(None) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            state.metrics.record_request();
            let response = Response::json(
                400,
                error_json(CODE_BAD_REQUEST, "Malformed request headers", &request_id),
            );
            return write_response(&mut stream, &request_id, &response);
        }
        Err(_) => return Ok(()),
    };

    if head.has_chunked_body {
        state.metrics.record_request();
        let response = Response::json(
            400,
            error_json(
                CODE_BAD_REQUEST,
                "Chunked transfer encoding is not supported; send Content-Length",
                &request_id,
            ),
        );
        return write_response(&mut stream, &request_id, &response);
    }

    if head.method == "POST" && head.content_length > MAX_BODY_BYTES {
        state.metrics.record_request();
        let response = Response::json(
            400,
            error_json(CODE_INVALID_JSON, "Request body too large", &request_id),
        );
        return write_response(&mut stream, &request_id, &response);
    }

    // Read exactly `content_length` bytes without pre-zeroing the full
    // allocation up front: `take` bounds the read and the length check turns
    // short bodies into 400 instead of dispatching garbage.
    let mut body = Vec::new();
    if head.method == "POST" && head.content_length > 0 {
        body.reserve(head.content_length.min(64 * 1024));
        let truncated = reader
            .by_ref()
            .take(head.content_length as u64)
            .read_to_end(&mut body)
            .is_err()
            || body.len() != head.content_length;
        if truncated {
            state.metrics.record_request();
            let response = Response::json(
                400,
                error_json(CODE_INVALID_JSON, "Truncated request body", &request_id),
            );
            return write_response(&mut stream, &request_id, &response);
        }
    }

    let response = dispatch(&head.method, &head.path, &body, &request_id, state);
    write_response(&mut stream, &request_id, &response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_cap_rejects_over_limit() {
        let metrics = crate::http::ServerMetrics::new();
        let mut guards = Vec::new();
        for _ in 0..MAX_CONCURRENT_CONNECTIONS {
            guards.push(ActiveGuard::enter(&metrics).expect("slot under cap"));
        }
        assert_eq!(metrics.active(), MAX_CONCURRENT_CONNECTIONS);
        assert!(ActiveGuard::enter(&metrics).is_none());
        assert_eq!(metrics.rejected(), 1);
        drop(guards);
        assert_eq!(metrics.active(), 0);
    }

    /// Feed raw bytes through the request-head parser over a loopback socket.
    fn head_for(request: &[u8]) -> std::io::Result<Option<RequestHead>> {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (server, _) = listener.accept().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client.write_all(request).unwrap();
        let mut reader = BufReader::new(server);
        read_request_head(&mut reader)
    }

    #[test]
    fn oversize_header_count_is_rejected() {
        let mut request = b"GET /health HTTP/1.1\r\n".to_vec();
        for i in 0..(MAX_HEADER_COUNT + 100) {
            request.extend_from_slice(format!("X-Pad-{}: abcdefgh\r\n", i).as_bytes());
        }
        request.extend_from_slice(b"\r\n");
        let err = head_for(&request).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn duplicate_content_length_is_rejected() {
        let err = head_for(
            b"POST /estimate-weight HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
        )
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn invalid_content_length_is_rejected() {
        let err = head_for(b"POST /estimate-weight HTTP/1.1\r\nContent-Length: abc\r\n\r\n{}")
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn chunked_encoding_is_flagged() {
        let head =
            head_for(b"POST /estimate-weight HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
                .unwrap()
                .unwrap();
        assert!(head.has_chunked_body);
    }
}
