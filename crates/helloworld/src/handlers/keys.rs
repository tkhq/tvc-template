use crate::{response::AppError, state::AppState};
use axum::{Json, extract::State};
use qos_core::protocol::services::boot::VersionedManifestEnvelope;
use qos_nsm::types::{NsmRequest, NsmResponse};
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttestationManifestEnvelope {
    #[serde(flatten)]
    envelope: VersionedManifestEnvelope,
    #[serde(with = "qos_hex::serde")]
    manifest_hash: Vec<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttestationResponse {
    manifest_envelope: AttestationManifestEnvelope,
    #[serde(with = "qos_hex::serde")]
    attestation_doc: Vec<u8>,
}

pub(crate) async fn attestation(
    State(state): State<AppState>,
) -> Result<Json<AttestationResponse>, AppError> {
    let manifest_bytes = std::fs::read(&state.manifest_file)
        .map_err(|e| AppError::internal(format!("failed to load manifest envelope: {e}")))?;
    let manifest_envelope = VersionedManifestEnvelope::try_from_slice_compat(&manifest_bytes)
        .map_err(|e| AppError::internal(format!("failed to decode manifest envelope: {e}")))?;
    let manifest_hash = manifest_envelope.manifest_hash();
    let ephemeral_key = state
        .ephemeral_key_handle
        .get_ephemeral_key()
        .map_err(|e| AppError::internal(format!("failed to load ephemeral key: {e}")))?;

    let nsm_response = state
        .nsm_provider
        .nsm_process_request(NsmRequest::Attestation {
            user_data: Some(manifest_hash.to_vec()),
            nonce: None,
            public_key: Some(ephemeral_key.public_key().to_bytes()),
        });
    let NsmResponse::Attestation { document } = nsm_response else {
        return Err(AppError::internal(format!(
            "unexpected NSM response: {nsm_response:?}"
        )));
    };

    Ok(Json(AttestationResponse {
        manifest_envelope: AttestationManifestEnvelope {
            envelope: manifest_envelope,
            manifest_hash: manifest_hash.to_vec(),
        },
        attestation_doc: document,
    }))
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

    let ephemeral_key = state
        .ephemeral_key_handle
        .get_ephemeral_key()
        .map_err(|e| AppError::internal(format!("failed to load ephemeral key: {e}")))?;
    let signature = ephemeral_key
        .sign(&payload_bytes)
        .map_err(|e| AppError::internal(format!("failed to sign proof payload: {e:?}")))?;
    let payload = String::from_utf8(payload_bytes)
        .map_err(|e| AppError::internal(format!("failed to encode proof payload: {e}")))?;

    let response = RandomAppProofResponse {
        payload: proof_payload,
        proof: AppProof {
            public_key: ephemeral_key.public_key().to_bytes(),
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
    let quorum_key = state
        .quorum_key_handle
        .get_quorum_key()
        .map_err(|e| AppError::internal(format!("failed to load quorum key: {e}")))?;
    let ciphertext = quorum_key
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
    let quorum_key = state
        .quorum_key_handle
        .get_quorum_key()
        .map_err(|e| AppError::internal(format!("failed to load quorum key: {e}")))?;
    let plaintext = quorum_key
        .decrypt(&ciphertext)
        .map_err(|e| AppError::bad_request(format!("failed to decrypt ciphertext: {e:?}")))?;
    let plaintext = String::from_utf8(plaintext.to_vec())
        .map_err(|e| AppError::bad_request(format!("decrypted plaintext is not UTF-8: {e}")))?;

    Ok(Json(QuorumKeyDecryptResponse { plaintext }))
}
