//! Rust backend for the cow weight estimator.
//!
//! Replaces the old Python HTTP server (`aif/server.py`): a threaded
//! HTTP/1.1 server on `std::net` with an Ollama Cloud client, a
//! deterministic fallback backend, image validation, result caching, and
//! retry-with-backoff — same API contract, no async runtime.

pub mod cache;
pub mod config;
pub mod fallback;
pub mod http;
pub mod ollama;
pub mod parse;
pub mod validate;
