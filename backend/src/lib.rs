//! Rust backend for the cow weight estimator.
//!
//! Threaded HTTP/1.1 server on `std::net` with an Ollama Cloud client, a
//! deterministic fallback backend, image validation, result caching, and
//! retry-with-backoff — same API contract, no async runtime.

pub mod args;
pub mod auth;
pub mod cache;
pub mod cli;
pub mod config;
pub mod db;
pub mod eval;
pub mod fallback;
pub mod hash;
pub mod http;
pub mod limits;
pub mod ollama;
pub mod parse;
pub mod tape;
pub mod time_util;
pub mod validate;
