# Store Interruption Recovery v1

S7E defines recovery at each durable boundary of a Store installation. Recovery
never trusts process memory, a partial file name supplied by a client, or an
ambiguous appd response. A retry always starts with a new S7D preflight and
repeats policy, capacity, Catalog, package, and appd verification.

## Download and power loss

Package partials remain private `0600` regular files named only by the signed
package SHA-256. A daemon restart has no persistent queued, downloading,
installing, or consent state. The Shell therefore returns to the Catalog's
authoritative available/update state, and a new explicit installation may reuse
only the bytes found under the same signed digest.

An interrupted write can leave any prefix length, including zero or the full
declared size. The downloader bounds that length before transfer. A short
response remains a network failure and preserves the bounded prefix. A partial
larger than the signed size is truncated before a fresh transfer. The complete
file is always hashed again before handoff, so a torn or corrupted prefix can
only cause verification failure, never installation.

## HTTP recovery

For a nonempty partial, the client requests `Range: bytes=N-` with identity
encoding. A `206` response must contain exactly `Content-Range: bytes
N-(total-1)/total`; missing, malformed, shifted, injected, or different-total
headers are rejected before response bytes are appended. Other non-success
statuses are network failures.

A server may ignore Range and return `200`. In that case the client truncates
the old prefix and consumes the response from byte zero. This is safe but loses
resume efficiency. Every response remains bounded by the signed package size,
and the final SHA-256 check is unchanged.

## Digest failure

Size and SHA-256 are checked from the staged partial immediately before appd
handoff. A digest mismatch synchronously truncates the partial to zero and
reports the closed `verification` failure reason. No handoff file is created,
and a later explicit retry downloads from zero. Truncation failure is still a
verification failure and cannot continue to appd.

## appd handoff

`cp0-stored` copies a verified package to a private, generated
`store-PID-SEQUENCE.capp` file and syncs it before sending the catalog-bound
owner, App ID, version, digest, and byte count to appd. A normal response or a
reported failure removes the handoff file.

The ambiguous case is appd committing the atomic install before its connection
to `cp0-stored` closes. A retry of the exact installed version is therefore an
idempotent replay, not a downgrade: appd still rechecks file ownership, length,
SHA-256, Store signature, manifest identity, SDK compatibility, and the exact
installed tree before returning success. Lower versions and different content
at the same version remain conflicts. A replay does not change install time or
rollback history and is accepted even if the recovered application has since
started.

On daemon startup, `cp0-stored` validates that the dedicated handoff directory
is private and owned by its effective UID. It removes only strict generated
handoff names. Unknown files are preserved, and a generated-looking directory
fails startup instead of triggering recursive deletion. A failed staging copy
also removes its incomplete destination immediately.

## Verification

Deterministic tests cover:

- a digest-bound partial consumed by a new Store service instance;
- a mid-response network disconnect followed by restart and exact-prefix reuse;
- a real loopback HTTP `206` with the wrong range rejected before append, then a
  correct range completing the same partial;
- wrong complete bytes truncated to zero, no installer call, and a clean retry;
- appd commit followed by connection loss and an exact idempotent replay;
- bounded startup cleanup that preserves unrelated inbox entries;
- exact replay, strict upgrade, downgrade, and invalid-version decisions in
  appd.

The CM0 gate repeats these behaviors with the production HTTPS origin and
systemd services after the stability observation window. Physical power loss,
link removal, appd kill timing, LCD progress, installed registry state, and SD
filesystem evidence remain required S9 artifacts; local process tests do not
substitute for those hardware results.
