# Store Review Risk Policy

Store risk is reviewer guidance derived from the immutable Scanner result. It is
not supplied by the developer or calculated by the Review Console.

## Policy version 1

The policy classifies declared SDK permissions after package signature, manifest
and WASM import validation:

| Tier | Conditions |
| --- | --- |
| `standard` | No sensitive permission, or only playback/notification output |
| `elevated` | Exactly one of network, user documents, microphone or camera |
| `high` | GPIO, LoRa, or two or more sensitive permissions |

Stable reason codes identify every contributing sensitive permission. A
`multiple-sensitive-capabilities` reason is added when at least two are present.
Reason codes are unique and sorted so the assessment has one canonical form.

## Trust and persistence

`cp0-store-risk` is the single Rust policy implementation shared by the isolated
Scanner and control API. A successful `ready-for-review` scan must carry exactly
one policy-v1 assessment. The Scan Worker inserts it in the same serializable
transaction as the append-only scan result and Submission state transition.

PostgreSQL stores assessments in `submission_risk_assessments`, binds each one to
the exact scan and report SHA-256, and re-evaluates policy v1 in a trigger. Direct
SQL cannot forge a tier, reorder reason codes, bind another report, update or
delete an assessment. Migration `0013` deterministically backfills policy v1 for
existing reviewable scans.

The Review Queue returns the newest stored policy version as `risk`; missing or
invalid assessments fail closed and do not become claimable queue entries.
Future policy changes append a new version instead of rewriting review history.

## Verification

```sh
cargo test -p cp0-store-risk -p cp0-store-scan -p cp0-store-scan-worker
cargo clippy -p cp0-store-risk -p cp0-store-scan -p cp0-store-scan-worker \
  --all-targets -- -D warnings

CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

The PostgreSQL gate covers atomic creation, migration backfill, canonical tiers
and reasons, append-only enforcement, forged policy/report rejection, Review
Queue serialization and rollback with the surrounding scan transaction.
