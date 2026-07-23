use crate::{response::AppError, state::AppState};
use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use turnkey_api_key_stamper::Stamp;
use turnkey_client::generated::{
    external::activity::v1::SignTransactionRequest,
    immutable::{activity::v1::SignTransactionIntentV2, common::v1::TransactionType},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnkeySignTransactionRequest {
    organization_id: String,
    sign_with: String,
    r#type: TransactionType,
    unsigned_transaction: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnkeySignTransactionResponse {
    activity_body: String,
    x_stamp: String,
}

pub(crate) async fn turnkey_sign_transaction(
    State(state): State<AppState>,
    request: Result<Json<TurnkeySignTransactionRequest>, JsonRejection>,
) -> Result<Json<TurnkeySignTransactionResponse>, AppError> {
    let Json(request) =
        request.map_err(|e| AppError::bad_request(format!("invalid request body: {e}")))?;
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| AppError::internal(format!("system clock error: {e}")))?
        .as_millis();
    let activity = SignTransactionRequest {
        r#type: "ACTIVITY_TYPE_SIGN_TRANSACTION_V2".to_owned(),
        timestamp_ms: timestamp_ms.to_string(),
        organization_id: request.organization_id,
        parameters: Some(SignTransactionIntentV2 {
            sign_with: request.sign_with,
            unsigned_transaction: request.unsigned_transaction,
            r#type: request.r#type,
        }),
        generate_app_proofs: None,
    };
    let activity_bytes = serde_json::to_vec(&activity)
        .map_err(|e| AppError::internal(format!("failed to serialize Turnkey activity: {e}")))?;

    let x_stamp = state
        .turnkey_api_key
        .stamp(&activity_bytes)
        .map_err(|e| AppError::internal(format!("failed to stamp Turnkey activity: {e}")))?
        .value;
    let activity_body = String::from_utf8(activity_bytes)
        .map_err(|e| AppError::internal(format!("failed to encode Turnkey activity: {e}")))?;

    Ok(Json(TurnkeySignTransactionResponse {
        activity_body,
        x_stamp,
    }))
}
