# Turnkey API Integration from TVC Apps

When a TVC application or its deployment scripts need to call the Turnkey API directly (user management, key creation, policy checks), use these patterns.

## API Base URLs

| Environment | Base URL |
|---|---|
| Production (default) | `https://api.turnkey.com` |
| Dev | `https://api.dev.turnkey.engineering` |

The dev environment requires a non-default `User-Agent` header. Cloudflare blocks standard library user agents (Python's `urllib`, etc.). Set `User-Agent: turnkey-cli/1.0` or any non-default value. Production does not have this restriction.

## Calling the Turnkey API from Shell Scripts

Use this pattern alongside TVC deployment scripts. Requires: `openssl`, `xxd`, `jq`, `curl`, and `python3` with `cryptography` for PEM conversion.

### One-time setup: convert hex private key to PEM

```bash
python3 -c "
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives import serialization
import sys
private_value = int('$TURNKEY_API_PRIVATE_KEY', 16)
private_key = ec.derive_private_key(private_value, ec.SECP256R1())
pem = private_key.private_bytes(
    serialization.Encoding.PEM,
    serialization.PrivateFormat.TraditionalOpenSSL,
    serialization.NoEncryption()
)
sys.stdout.buffer.write(pem)
" > /tmp/tk-private.pem
```

### Stamp and call pattern

```bash
# Build request body
BODY='{"organizationId":"'"$TURNKEY_ORGANIZATION_ID"'"}'

# Sign with ECDSA P-256 SHA-256, DER output, hex-encoded
SIGNATURE_HEX=$(echo -n "$BODY" | \
  openssl dgst -sha256 -sign /tmp/tk-private.pem | \
  xxd -p -c 256)

# Build stamp JSON
STAMP_JSON=$(jq -cn \
  --arg pk "$TURNKEY_API_PUBLIC_KEY" \
  --arg sig "$SIGNATURE_HEX" \
  '{publicKey: $pk, signature: $sig, scheme: "SIGNATURE_SCHEME_TK_API_P256"}')

# Base64URL encode (no padding, no line breaks)
STAMP=$(echo -n "$STAMP_JSON" | base64 | tr '+/' '-_' | tr -d '=' | tr -d '\n')

# Make the request
curl -s -X POST \
  -H "Content-Type: application/json" \
  -H "X-Stamp: $STAMP" \
  -d "$BODY" \
  "https://api.turnkey.com/public/v1/query/whoami" | jq .
```

Each request needs a fresh stamp since the signature covers the exact request body.

### Example: create a user with an API key

Users require at least one credential (API key or authenticator) at creation time. You cannot create a bare user without credentials.

```bash
# Generate a P-256 key pair for the new user
openssl ecparam -name prime256v1 -genkey -noout -out /tmp/new-user-key.pem 2>/dev/null
NEW_PUB_HEX=$(openssl ec -in /tmp/new-user-key.pem -pubout -conv_form compressed -outform DER 2>/dev/null | tail -c 33 | xxd -p -c 33)

TIMESTAMP_MS=$(date +%s)000
BODY=$(jq -cn \
  --arg ts "$TIMESTAMP_MS" \
  --arg org "$TURNKEY_ORGANIZATION_ID" \
  --arg pub "$NEW_PUB_HEX" \
  '{
    type: "ACTIVITY_TYPE_CREATE_USERS_V2",
    timestampMs: $ts,
    organizationId: $org,
    parameters: {
      users: [{
        userName: "new-user",
        userEmail: "user@example.com",
        apiKeys: [{
          apiKeyName: "user-key",
          publicKey: $pub,
          curveType: "API_KEY_CURVE_P256"
        }],
        authenticators: [],
        userTags: []
      }]
    }
  }')

# Sign and stamp (same pattern as above)
SIGNATURE_HEX=$(echo -n "$BODY" | openssl dgst -sha256 -sign /tmp/tk-private.pem | xxd -p -c 256)
STAMP_JSON=$(jq -cn --arg pk "$TURNKEY_API_PUBLIC_KEY" --arg sig "$SIGNATURE_HEX" \
  '{publicKey: $pk, signature: $sig, scheme: "SIGNATURE_SCHEME_TK_API_P256"}')
STAMP=$(echo -n "$STAMP_JSON" | base64 | tr '+/' '-_' | tr -d '=' | tr -d '\n')

curl -s -X POST \
  -H "Content-Type: application/json" \
  -H "X-Stamp: $STAMP" \
  -d "$BODY" \
  "https://api.turnkey.com/public/v1/submit/create_users" | jq '{status: .activity.status, userIds: .activity.result.createUsersResult.userIds}'
```

## Calling the Turnkey API from Rust (inside a TVC app)

For TVC apps that need to call the Turnkey API at runtime (e.g., a policy-gated signer that checks user permissions), construct the stamp in Rust:

```rust
use p256::ecdsa::{SigningKey, Signature, signature::Signer};
use sha2::{Sha256, Digest};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;

fn stamp_request(body: &str, private_key_hex: &str, public_key_hex: &str) -> String {
    let key_bytes = hex::decode(private_key_hex).expect("valid hex");
    let signing_key = SigningKey::from_bytes((&key_bytes[..]).into()).expect("valid key");

    let signature: Signature = signing_key.sign(body.as_bytes());
    let der_sig = signature.to_der();
    let sig_hex = hex::encode(der_sig.as_bytes());

    let stamp = json!({
        "publicKey": public_key_hex,
        "signature": sig_hex,
        "scheme": "SIGNATURE_SCHEME_TK_API_P256"
    });

    URL_SAFE_NO_PAD.encode(stamp.to_string().as_bytes())
}
```

Required workspace dependencies:

```toml
p256 = { version = "0.13", features = ["ecdsa"] }
sha2 = "0.10"
hex = "0.4"
base64 = "0.22"
```

Use `reqwest` with `rustls-tls` to make the HTTP call (consistent with existing TVC app patterns).

## Common Gotchas

- **User creation requires credentials**: The `create_users` endpoint returns a 400 if any user in the batch has no API keys and no authenticators.
- **Dev environment Cloudflare**: Requests to `api.dev.turnkey.engineering` without a custom `User-Agent` header get blocked with HTTP 403 / error code 1010.
- **Stamp is per-request**: The signature covers the exact request body bytes. You cannot reuse a stamp across different requests.
- **Activity polling**: Submit endpoints return an activity object. If `status` is not `ACTIVITY_STATUS_COMPLETED`, poll the activity until it completes before using the result.

## Related Skills

- `stamping-api` for detailed stamp construction reference (all languages, WebAuthn variant)
- `managing-users-api` for the full user/API-key/tag management API surface
