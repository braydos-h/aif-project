//! Small JSON-body helpers shared by the account-data handlers.

use serde_json::Value;

use super::response::{error_json, Response};

/// Parse a JSON body or answer `400 missing_body` / `400 invalid_json`.
pub(crate) fn need_json(
    state: &super::ServerState,
    body: &[u8],
    request_id: &str,
) -> Result<Value, Response> {
    if body.is_empty() {
        return Err(Response::json(
            400,
            error_json(
                super::response::CODE_MISSING_BODY,
                "Missing request body",
                request_id,
            ),
        )
        .with_policy(state.config.production));
    }
    serde_json::from_slice(body).map_err(|_| {
        Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_JSON,
                "Invalid JSON payload",
                request_id,
            ),
        )
        .with_policy(state.config.production)
    })
}

/// Optional string field with a 400 on wrong types.
pub(crate) fn opt_str<'a>(
    state: &super::ServerState,
    payload: &'a Value,
    field: &str,
    request_id: &str,
) -> Result<Option<&'a str>, Response> {
    super::validation::optional_string(payload, field).map_err(|message| {
        Response::json(
            400,
            error_json(super::response::CODE_INVALID_OPTIONS, &message, request_id),
        )
        .with_policy(state.config.production)
    })
}

/// Optional numeric field with a 400 on wrong types.
pub(crate) fn opt_num(
    state: &super::ServerState,
    payload: &Value,
    field: &str,
    request_id: &str,
) -> Result<Option<f64>, Response> {
    super::validation::optional_number(payload, field).map_err(|message| {
        Response::json(
            400,
            error_json(super::response::CODE_INVALID_OPTIONS, &message, request_id),
        )
        .with_policy(state.config.production)
    })
}
