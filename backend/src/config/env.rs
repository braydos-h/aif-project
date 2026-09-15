//! `.env` file loading and environment variable helpers.
//!
//! Values already present in the process environment take precedence over
//! `.env` (same contract as `aif/config.py`).

use std::env;
use std::path::Path;

/// Locations probed for `.env`, in priority order: current directory,
/// directory next to the running executable (installed binaries), then
/// the source-tree root. Mirrors `aif/config.py::_env_candidates`
/// (which probes exe-dir then repo root).
pub fn env_candidates(filename: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    out.push(
        std::env::current_dir()
            .unwrap_or_else(|_| ".".into())
            .join(filename),
    );
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join(filename));
        }
    }
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."));
    out.push(repo_root.join(filename));
    out
}

/// Parse `.env` content without touching the environment (shared by the
/// loader and tests).
pub(crate) fn apply_env_content(content: &str) {
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap();
        let key = key.trim();
        let mut value = value.trim().to_string();
        if value.len() >= 2 {
            let first = value.chars().next().unwrap();
            let last = value.chars().last().unwrap();
            if first == last && (first == '\'' || first == '"') {
                value = value[1..value.len() - 1].to_string();
            }
        }
        if !key.is_empty() && env::var(key).is_err() {
            env::set_var(key, &value);
        }
    }
}

/// Load a `.env` file into the environment without overriding existing values.
///
/// Probes [`env_candidates`] in order; the first file found wins. Lines like
/// `KEY=value` are parsed; blank lines and `#` comments are ignored. Quoted
/// values have the quotes stripped.
pub fn load_env_file(filename: &str) {
    for path in env_candidates(filename) {
        if !path.is_file() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            apply_env_content(&content);
            return;
        }
    }
}

/// Read an env var, falling back to `default` when unset or empty.
pub fn env_or(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_env_file(path: &str) {
        let content = std::fs::read_to_string(Path::new(path)).unwrap();
        apply_env_content(&content);
    }

    #[test]
    fn env_file_loads_into_missing_vars_without_overriding() {
        let dir = std::env::temp_dir().join(format!("aif-env-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".env"),
            "AIF_AI_BACKEND=none\n# comment\n\nAIF_TEST_QUOTED=\"hello world\"\n",
        )
        .unwrap();

        // Existing values take precedence over .env.
        env::set_var("AIF_AI_BACKEND", "ollama");
        env::remove_var("AIF_TEST_QUOTED");
        parse_env_file(dir.join(".env").to_str().unwrap());

        assert_eq!(env::var("AIF_AI_BACKEND").unwrap(), "ollama");
        assert_eq!(env::var("AIF_TEST_QUOTED").unwrap(), "hello world");
        env::remove_var("AIF_AI_BACKEND");
        env::remove_var("AIF_TEST_QUOTED");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn env_candidates_prefer_cwd_then_exe_dir() {
        let c = env_candidates(".env");
        assert!(c.len() >= 2);
        assert_eq!(c[0].file_name().unwrap(), ".env");
    }
}
