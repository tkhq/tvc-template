# Application Examples

Concrete endpoint designs for common TVC application scenarios. Each example shows the route handler pattern, request/response shapes, and key implementation notes.

## Verifiable Price Oracle

**Target audience:** Both (DeFi protocols need trusted price feeds, banks need fair value calculations)

**Value proposition:** Price computed inside a tamper-proof enclave from multiple sources, signed with App Proof. No operator can manipulate the price.

### Endpoints

```
GET /price/{asset}         -> Fetch aggregated price for an asset
GET /price/{asset}/sources -> Show individual source prices + methodology
GET /health                -> (provided by template)
GET /metrics               -> (provided by template)
```

### Route Handler Pattern

```rust
use axum::{Router, extract::Path, response::IntoResponse, routing::get};
use serde_json::json;

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/price/{asset}", get(get_price))
        .route("/price/{asset}/sources", get(get_price_sources))
        .layer(TraceLayer::new_for_http())
}

async fn get_price(Path(asset): Path<String>) -> impl IntoResponse {
    // Fetch from multiple sources
    let sources = fetch_prices(&asset).await;
    match sources {
        Ok(prices) => {
            let median = compute_median(&prices);
            axum::Json(json!({
                "asset": asset,
                "price": median,
                "sources_count": prices.len(),
                "methodology": "median",
                "timestamp": current_timestamp()
            }))
            .into_response()
        }
        Err(e) => (
            axum::http::StatusCode::BAD_GATEWAY,
            axum::Json(json!({"error": format!("failed to fetch prices: {e}")})),
        )
            .into_response(),
    }
}
```

### Dependencies to Add

```toml
# src/Cargo.toml [workspace.dependencies]
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }

# src/helloworld/Cargo.toml [dependencies]
reqwest = { workspace = true }
```

### Implementation Notes

- Use `reqwest` with `rustls-tls` (not `native-tls`) since the enclave has no system SSL libraries
- Fetch from 3+ sources and take the median to handle stale/outlier data
- Return source-level data at `/sources` for transparency
- Handle partial failures gracefully (if 1 of 3 sources fails, use remaining 2)

---

## Prediction Market Settlement Engine

**Target audience:** Web3, prediction markets

**Value proposition:** Market resolution logic runs in an enclave. The outcome is provably computed from the stated data sources. No insider can rig results.

### Endpoints

```
POST /market/create        -> Define a market (question, resolution sources, rules)
POST /market/resolve       -> Trigger resolution: fetch data, apply rules, sign result
GET  /market/{id}          -> Get market status and resolution details
GET  /health               -> (provided by template)
GET  /metrics              -> (provided by template)
```

### Implementation Notes

- Store market definitions in memory (HashMap) for simplicity. No persistent storage in enclaves.
- Resolution logic must be deterministic: same inputs always produce the same outcome
- Return all source data alongside the outcome for auditability
- Consider a "dry run" mode that shows what the resolution would be without finalizing

---

## Policy-Gated Transaction Signer

**Target audience:** Both (web3 treasury management, bank transaction authorization)

**Value proposition:** The enclave enforces spending policies before signing. Every decision produces a verifiable Policy Outcome Proof. Maps directly to Turnkey's core product.

### Endpoints

```
POST /policy/configure     -> Set policies (limits, allowlists, schedules)
POST /sign                 -> Submit transaction for policy evaluation + signing
GET  /policy               -> View current policy configuration
GET  /audit                -> View recent signing decisions with proofs
GET  /health               -> (provided by template)
GET  /metrics              -> (provided by template)
```

### Policy Types to Implement

- **Spending limits**: Per-transaction max, daily aggregate max
- **Allowlisted destinations**: Only sign transactions to known addresses
- **Time restrictions**: No signing outside business hours
- **Amount thresholds**: Require additional approval above $X

---

## Sealed-Bid Auction

**Target audience:** Both (NFT auctions, treasury auctions, ad-tech)

**Value proposition:** No one, including the operator, can see bids before the reveal. The auction result is cryptographically verifiable. Great for live demos.

### Endpoints

```
POST /auction/create       -> Create an auction (item, deadline)
POST /auction/{id}/bid     -> Submit an encrypted bid
POST /auction/{id}/reveal  -> Trigger reveal + determine winner (after deadline)
GET  /auction/{id}         -> Get auction status
GET  /health               -> (provided by template)
GET  /metrics              -> (provided by template)
```

### Implementation Notes

- For simplicity, bids can be plaintext JSON (simulating encryption). A production version would use the enclave's public key for actual encryption.
- Store auctions and bids in memory (HashMap)
- Reject bids after the deadline
- On reveal: sort bids, determine winner, return all bids for transparency
- Include bid count and timestamp in the auction status (without revealing bid values)

---

## Timestamp Notary Service

**Target audience:** Both (document verification, proof of existence, audit trails)

**Value proposition:** Proves that a document hash existed at a specific point in time. The enclave provides a tamper-proof timestamp, eliminating the need to trust a third-party timestamping authority.

### Endpoints

```
POST /notarize             -> Submit a hash, get a timestamped receipt
GET  /verify/{receipt_id}  -> Look up a receipt by ID
GET  /stats                -> Total notarized documents
GET  /health               -> (provided by template)
GET  /metrics              -> (provided by template)
```

### Implementation Notes

- Use `Arc<RwLock<...>>` for shared state (receipts stored in memory)
- Sequential receipt numbers for easy lookup
- Return the hash, Unix timestamp, and receipt number on notarize
- Return 404 for unknown receipt IDs on verify
- The `/stats` endpoint provides transparency into service usage
