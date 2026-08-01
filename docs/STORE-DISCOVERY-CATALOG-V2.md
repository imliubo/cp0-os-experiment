# Store Discovery Catalog v2

Catalog v2 adds signed discovery metadata without changing the package install
trust chain. It is the first S6 slice toward an App Store-style browse and search
experience on the 320x170 device.

## Compatibility contract

`cp0-store-protocol` accepts exactly two Catalog schemas:

- v1 contains the original package, summary and permission fields and must not
  contain `discovery`;
- v2 requires `discovery` on every application;
- any other schema, a v1/v2 field mix, unknown field or missing v2 value fails
  closed before the Catalog can replace the verified cache.

The signature envelope and anti-rollback sequence remain unchanged. The schema
version is inside the signed Catalog, so a CDN cannot add, remove or rewrite
discovery fields. The offline `cp0ctl store publish` fixture builder continues to
emit v1 for recovery compatibility. A production projection emits v2 once every
included package artifact has complete signed discovery data. During an upgrade,
any remaining legacy artifact keeps the whole snapshot on pure v1; the Publisher
strips v2 fields instead of emitting a mixed or unavailable Catalog.

## Signed discovery fields

Each v2 application adds:

| Field | Source | Bound |
| --- | --- | --- |
| `developer` | owner Team display name at publication | 1-80 safe characters |
| `subtitle` | approved default Listing localization | 1-48 safe characters; must equal `summary` |
| `category` | approved Listing category | closed eight-value enum |
| `keywords` | approved default Listing localization | at most eight, unique and sorted |
| `age_rating` | approved Listing | `4+`, `9+`, `12+` or `17+` |
| `privacy_url` | approved Listing | bounded HTTPS URL |
| `support_url` | approved Listing | bounded HTTPS URL |

The Publisher reconstructs the developer-signed package, Listing and every
asset from immutable content-addressed upload parts, verifies their digests and
the independent double approval, then creates the Store-signed package and
Catalog v2 in one immutable generation. It never accepts discovery metadata
from an editorial override or an unsigned request field.

## Device behavior

`cp0-stored` verifies and caches v1 and v2 with the same key, validity window and
sequence protections. Existing name ranking remains stable. For v2, local search
also matches the signed developer name, category and keywords; an exact keyword
match ranks ahead of general metadata containment. Search text remains local and
is never sent to the Store origin.

Catalog responses to the current System Shell remain the bounded v1 summary
shape, so this change does not increase the C UI allocation or frame size. S6B
now publishes signed icon/screenshot resources through Catalog v3; caching and
rendering those resources, localized selection, Today collections and signed
S6E now adds the compatible signed root/category index and bounded shards; see
`STORE-SHARDED-CATALOG-V1.md`.

## Verification

```sh
cargo test -p cp0-store-protocol -p cp0-stored
cargo check -p cp0-store-publisher --all-targets

CP0_STORE_TEST_DATABASE_URL=postgres://... \
  cargo test -p cp0-store-publisher --test postgres -- --ignored --nocapture
```

The protocol tests cover v1/v2 separation, missing discovery data, noncanonical
keywords, subtitle mismatch and signature tampering. The device service tests
cover v1 ranking compatibility and v2 developer/category/keyword search. The
PostgreSQL Publisher gate proves that v2 values come from the approved Listing
and Team while the generated package and Catalog remain reproducible and signed.
Publisher unit coverage also proves that mixed legacy projections stay pure v1.
