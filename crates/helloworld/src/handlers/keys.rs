use crate::{response::AppError, state::AppState};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct RandomNumberProofPayload {
    // Additional metadata can be added here later if the proof needs stronger
    // domain separation or audit context.
    #[serde(with = "qos_json::string_or_numeric")]
    random_number: u64,
}

#[derive(Serialize)]
struct AppProof {
    // The enclave's ephemeral public key used to generate the signature.
    #[serde(with = "qos_hex::serde")]
    public_key: Vec<u8>,
    // The exact serialized payload is included so clients can verify the
    // signature without extra deterministic serialization logic.
    payload: String,
    // The ephemeral key's signature over the payload.
    #[serde(with = "qos_hex::serde")]
    signature: Vec<u8>,
}

#[derive(Serialize)]
pub(crate) struct RandomAppProofResponse {
    payload: RandomNumberProofPayload,
    proof: AppProof,
}

#[derive(Deserialize)]
pub(crate) struct QuorumKeyEncryptRequest {
    plaintext: String,
}

#[derive(Serialize)]
pub(crate) struct QuorumKeyEncryptResponse {
    #[serde(with = "qos_hex::serde")]
    ciphertext: Vec<u8>,
}

#[derive(Deserialize)]
pub(crate) struct QuorumKeyDecryptRequest {
    ciphertext: String,
}

#[derive(Serialize)]
pub(crate) struct QuorumKeyDecryptResponse {
    plaintext: String,
}

pub(crate) async fn random_app_proof(
    State(state): State<AppState>,
) -> Result<Json<RandomAppProofResponse>, AppError> {
    let random_number = rand::random::<u64>();
    let proof_payload = RandomNumberProofPayload { random_number };

    // QOS JSON is a deterministic serialization protocol with stricter rules
    // than normal JSON. It is useful when you need canonical serialization for
    // verifying signatures. We sign these exact bytes and return them in the response
    // to make it easy for clients to verify the signature.
    let payload_bytes = qos_json::to_vec(&proof_payload)
        .map_err(|e| AppError::internal(format!("failed to serialize proof payload: {e}")))?;

    let signature = state
        .ephemeral_key
        .sign(&payload_bytes)
        .map_err(|e| AppError::internal(format!("failed to sign proof payload: {e:?}")))?;
    let payload = String::from_utf8(payload_bytes)
        .map_err(|e| AppError::internal(format!("failed to encode proof payload: {e}")))?;

    let response = RandomAppProofResponse {
        payload: proof_payload,
        proof: AppProof {
            public_key: state.ephemeral_key.public_key().to_bytes(),
            payload,
            signature,
        },
    };

    Ok(Json(response))
}

pub(crate) async fn quorum_key_encrypt(
    State(state): State<AppState>,
    Json(request): Json<QuorumKeyEncryptRequest>,
) -> Result<Json<QuorumKeyEncryptResponse>, AppError> {
    let ciphertext = state
        .quorum_key
        .public_key()
        .encrypt(request.plaintext.as_bytes())
        .map_err(|e| AppError::internal(format!("failed to encrypt plaintext: {e:?}")))?;

    Ok(Json(QuorumKeyEncryptResponse { ciphertext }))
}

pub(crate) async fn quorum_key_decrypt(
    State(state): State<AppState>,
    Json(request): Json<QuorumKeyDecryptRequest>,
) -> Result<Json<QuorumKeyDecryptResponse>, AppError> {
    let ciphertext = qos_hex::decode(&request.ciphertext)
        .map_err(|e| AppError::bad_request(format!("invalid ciphertext hex: {e:?}")))?;
    let plaintext = state
        .quorum_key
        .decrypt(&ciphertext)
        .map_err(|e| AppError::bad_request(format!("failed to decrypt ciphertext: {e:?}")))?;
    let plaintext = String::from_utf8(plaintext.to_vec())
        .map_err(|e| AppError::bad_request(format!("decrypted plaintext is not UTF-8: {e}")))?;

    Ok(Json(QuorumKeyDecryptResponse { plaintext }))
}
