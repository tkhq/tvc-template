use crate::{response::AppError, state::AppState};
use axum::{Json, extract::State};
use base64::Engine;
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use qos_p256::P256Pair;
use serde::{Deserialize, Serialize};

const ACTIVITY_TYPE: &str = "ACTIVITY_TYPE_SIGN_TRANSACTION_V2";
const API_KEY_DERIVE_PATH: &[u8] = b"tvc-template-turnkey-api-key-v1";
const STAMP_SCHEME: &str = "SIGNATURE_SCHEME_TK_API_P256";
const TRANSACTION_TYPE: &str = "TRANSACTION_TYPE_ETHEREUM";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignTransactionParameters<'a> {
    sign_with: &'a str,
    unsigned_transaction: String,
    #[serde(rename = "type")]
    transaction_type: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignTransactionActivity<'a> {
    #[serde(rename = "type")]
    activity_type: &'static str,
    timestamp_ms: String,
    organization_id: &'a str,
    parameters: SignTransactionParameters<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StampEnvelope {
    public_key: String,
    scheme: &'static str,
    signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SignTurnkeyTransactionRequest {
    organization_id: String,
    sign_with: String,
    unsigned_transaction: String,
    timestamp_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SignTurnkeyTransactionResponse {
    activity_body: String,
    x_stamp: String,
}

fn build_activity_body(
    organization_id: &str,
    sign_with: &str,
    unsigned_transaction: &str,
    timestamp_ms: u64,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&SignTransactionActivity {
        activity_type: ACTIVITY_TYPE,
        timestamp_ms: timestamp_ms.to_string(),
        organization_id,
        parameters: SignTransactionParameters {
            sign_with,
            unsigned_transaction: unsigned_transaction
                .strip_prefix("0x")
                .or_else(|| unsigned_transaction.strip_prefix("0X"))
                .unwrap_or(unsigned_transaction)
                .to_ascii_lowercase(),
            transaction_type: TRANSACTION_TYPE,
        },
    })
}

fn stamp_activity_body(quorum_key: &P256Pair, body: &str) -> Result<String, String> {
    let api_key_secret = qos_p256::derive_secret(quorum_key.to_master_seed(), API_KEY_DERIVE_PATH)
        .map_err(|error| format!("failed to derive Turnkey API key: {error:?}"))?;
    let signing_key = SigningKey::from_slice(&api_key_secret[..])
        .map_err(|error| format!("failed to construct Turnkey API key: {error}"))?;
    let signature: Signature = signing_key.sign(body.as_bytes());
    let envelope = StampEnvelope {
        public_key: qos_hex::encode(
            signing_key
                .verifying_key()
                .to_encoded_point(true)
                .as_bytes(),
        ),
        scheme: STAMP_SCHEME,
        signature: qos_hex::encode(signature.to_der().as_bytes()),
    };
    let envelope = serde_json::to_vec(&envelope)
        .map_err(|error| format!("failed to serialize Turnkey stamp: {error}"))?;

    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(envelope))
}

pub(crate) async fn sign_turnkey_transaction(
    State(state): State<AppState>,
    Json(request): Json<SignTurnkeyTransactionRequest>,
) -> Result<Json<SignTurnkeyTransactionResponse>, AppError> {
    let activity_body = build_activity_body(
        &request.organization_id,
        &request.sign_with,
        &request.unsigned_transaction,
        request.timestamp_ms,
    )
    .map_err(|error| {
        AppError::internal(format!("failed to serialize Turnkey activity: {error}"))
    })?;
    let x_stamp =
        stamp_activity_body(&state.quorum_key, &activity_body).map_err(AppError::internal)?;

    Ok(Json(SignTurnkeyTransactionResponse {
        activity_body,
        x_stamp,
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use base64::Engine;
    use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
    use qos_p256::P256Pair;

    #[test]
    fn activity_body_matches_turnkey_sign_transaction_v2_shape() {
        let body = build_activity_body(
            "org-123",
            "0xabc0000000000000000000000000000000000001",
            "0xDEADBEEF",
            1_700_000_000_000,
        )
        .unwrap();
        let activity: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(activity["type"], "ACTIVITY_TYPE_SIGN_TRANSACTION_V2");
        assert_eq!(activity["timestampMs"], "1700000000000");
        assert_eq!(activity["organizationId"], "org-123");
        assert_eq!(
            activity["parameters"]["signWith"],
            "0xabc0000000000000000000000000000000000001"
        );
        assert_eq!(activity["parameters"]["unsignedTransaction"], "deadbeef");
        assert_eq!(activity["parameters"]["type"], "TRANSACTION_TYPE_ETHEREUM");
    }

    #[test]
    fn stamp_signs_exact_activity_body_with_derived_p256_key() {
        let quorum_key = P256Pair::generate().unwrap();
        let body = r#"{"type":"ACTIVITY_TYPE_SIGN_TRANSACTION_V2"}"#;
        let x_stamp = stamp_activity_body(&quorum_key, body).unwrap();
        let envelope_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(x_stamp)
            .unwrap();
        let envelope: serde_json::Value = serde_json::from_slice(&envelope_bytes).unwrap();

        assert_eq!(envelope["scheme"], "SIGNATURE_SCHEME_TK_API_P256");
        let public_key = qos_hex::decode(envelope["publicKey"].as_str().unwrap()).unwrap();
        let signature = qos_hex::decode(envelope["signature"].as_str().unwrap()).unwrap();
        let verifying_key = VerifyingKey::from_sec1_bytes(&public_key).unwrap();
        let signature = Signature::from_der(&signature).unwrap();

        verifying_key.verify(body.as_bytes(), &signature).unwrap();
        assert!(verifying_key.verify(b"modified body", &signature).is_err());
    }
}
