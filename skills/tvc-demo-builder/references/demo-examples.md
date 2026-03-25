# Demo Implementation Examples

Concrete endpoint designs for common TVC demo scenarios. Each example shows the route handler pattern, request/response shapes, and key implementation notes.

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

**Target audience:** Web3, prediction markets (Polymarket, Kalshi)

**Value proposition:** Market resolution logic runs in an enclave. The outcome is provably computed from the stated data sources. No insider can rig results.

### Endpoints

```
POST /market/create        -> Define a market (question, resolution sources, rules)
POST /market/resolve       -> Trigger resolution: fetch data, apply rules, sign result
GET  /market/{id}          -> Get market status and resolution details
GET  /health               -> (provided by template)
GET  /metrics              -> (provided by template)
```

### Route Handler Pattern

```rust
async fn resolve_market(
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> Response {
    let market_id = payload["market_id"].as_str();
    let Some(market_id) = market_id else {
        return (StatusCode::BAD_REQUEST, axum::Json(json!({"error": "missing market_id"}))).into_response();
    };

    // 1. Fetch outcome data from configured sources
    let source_data = fetch_resolution_data(market_id).await;

    // 2. Apply deterministic resolution rules
    let outcome = apply_resolution_rules(&source_data);

    // 3. Return signed settlement
    (StatusCode::OK, axum::Json(json!({
        "market_id": market_id,
        "outcome": outcome,
        "sources": source_data,
        "resolved_at": current_timestamp(),
        "methodology": "majority_of_sources"
    }))).into_response()
}
```

### Implementation Notes

- Store market definitions in memory (HashMap) for the demo. No persistent storage in enclaves.
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

### Route Handler Pattern

```rust
async fn sign_transaction(
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> Response {
    let amount = payload["amount"].as_f64();
    let destination = payload["destination"].as_str();

    // 1. Evaluate all policies
    let policy_result = evaluate_policies(&payload);

    match policy_result {
        PolicyOutcome::Allow { reasons } => {
            // 2. Sign the transaction
            (StatusCode::OK, axum::Json(json!({
                "decision": "ALLOW",
                "reasons": reasons,
                "signed": true,
                "timestamp": current_timestamp()
            }))).into_response()
        }
        PolicyOutcome::Deny { violations } => {
            (StatusCode::FORBIDDEN, axum::Json(json!({
                "decision": "DENY",
                "violations": violations,
                "signed": false,
                "timestamp": current_timestamp()
            }))).into_response()
        }
    }
}
```

### Policy Types to Implement

- **Spending limits**: Per-transaction max, daily aggregate max
- **Allowlisted destinations**: Only sign transactions to known addresses
- **Time restrictions**: No signing outside business hours
- **Amount thresholds**: Require additional approval above $X

### Implementation Notes

- Store policies in memory (loaded via `/policy/configure`)
- Track daily totals in memory for aggregate limit enforcement
- Return detailed violation messages on DENY (which policy, what the limit is, what was attempted)
- The audit log shows the full decision trail for each request

---

## Compliant Trade Execution Gate

**Target audience:** Banks, financial institutions (JP Morgan, Goldman)

**Value proposition:** Every trade passes through the enclave for compliance checks. The enclave produces cryptographic proof that the check occurred, what rules were applied, and the outcome. Auditable, tamper-proof compliance trail.

### Endpoints

```
POST /trade/check          -> Submit trade for compliance evaluation
GET  /trade/audit/{id}     -> Get compliance check details for a specific trade
POST /rules/update         -> Update compliance rule sets (sanctions, limits)
GET  /rules                -> View current rule configuration
GET  /health               -> (provided by template)
GET  /metrics              -> (provided by template)
```

### Route Handler Pattern

```rust
async fn check_trade(
    axum::Json(trade): axum::Json<serde_json::Value>,
) -> Response {
    let instrument = trade["instrument"].as_str().unwrap_or("unknown");
    let counterparty = trade["counterparty"].as_str().unwrap_or("unknown");
    let amount = trade["amount"].as_f64().unwrap_or(0.0);

    let mut checks = Vec::new();

    // 1. Sanctions screening
    let sanctions_result = check_sanctions(counterparty);
    checks.push(sanctions_result);

    // 2. Position limit check
    let position_result = check_position_limits(instrument, amount);
    checks.push(position_result);

    // 3. Wash trading detection
    let wash_result = check_wash_trading(instrument, counterparty);
    checks.push(wash_result);

    // 4. Restricted securities
    let restricted_result = check_restricted_list(instrument);
    checks.push(restricted_result);

    let all_passed = checks.iter().all(|c| c.passed);
    let status = if all_passed { StatusCode::OK } else { StatusCode::FORBIDDEN };

    (status, axum::Json(json!({
        "decision": if all_passed { "APPROVE" } else { "REJECT" },
        "checks": checks,
        "trade": trade,
        "evaluated_at": current_timestamp()
    }))).into_response()
}
```

### Implementation Notes

- Load sanctions lists and restricted securities from configuration at startup
- Each check should return its own pass/fail with a reason string
- Even approved trades should log the full check trail
- Include the rule version/hash in the response for audit reproducibility

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

- For the demo, bids can be plaintext JSON (simulating encryption). A production version would use the enclave's public key for actual encryption.
- Store auctions and bids in memory (HashMap)
- Reject bids after the deadline
- On reveal: sort bids, determine winner, return all bids for transparency
- Include bid count and timestamp in the auction status (without revealing bid values)
