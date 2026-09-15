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
use super::response::{error_json, write_response, Response, CODE_INVALID_JSON, CODE_SERVER_BUSY};
use super::validation::MAX_BODY_BYTES;
use super::{dispatch, ServerState};

/// Max simultaneous connections handled. Excess connections are refused
/// with `503 server_busy` so one burst cannot exhaust threads/memory.
/// 64 comfortably covers the WebUI (sequential batch + demo + tape) while
/// bounding worst-case thread use on small field machines.
pub const MAX_CONCURRENT_CONNECTIONS: usize = 64;

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
            Ok(stream) => {
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
struct RequestHead {
    method: String,
    path: String,
    content_length: usize,
}

fn read_request_head(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<RequestHead>> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None); // client closed before sending anything
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

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    Ok(Some(RequestHead {
        method,
        path,
        content_length,
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

    let Some(head) = read_request_head(&mut reader)? else {
        return Ok(());
    };

    if head.method == "POST" && head.content_length > MAX_BODY_BYTES {
        let response = Response::json(
            400,
            error_json(CODE_INVALID_JSON, "Request body too large", &request_id),
        );
        return write_response(&mut stream, &request_id, &response);
    }

    let mut body = Vec::new();
    if head.method == "POST" && head.content_length > 0 {
        body.resize(head.content_length, 0);
        if reader.read_exact(&mut body).is_err() {
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
    fn concurrency_cap_is_sane() {
        assert!(MAX_CONCURRENT_CONNECTIONS >= 16);
        assert!(MAX_CONCURRENT_CONNECTIONS <= 256);
    }
}
