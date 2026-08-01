# Store search privacy v1

S8D freezes Store search as an on-device operation. A query travels only from
the trusted System Shell to `cp0-stored` over the local Unix socket. The daemon
ranks applications from the already verified Catalog in memory and returns a
bounded page. Search never triggers Catalog, media, package or metrics network
traffic.

## Data boundary

- Queries are not written to daemon state, metrics state, logs or Catalog
  cache files.
- The Shell keeps at most four recent queries in its process-owned UI struct.
  They are lost when the Shell restarts and are never written to disk.
- The response echoes the exact query only to bind a local response to its
  request. The strict IPC decoder rejects a different query or page.
- Aggregate metrics contain only exact app version install, launch and crash
  counters. Their strict schema rejects `query`, `experiment_id` and every
  other unknown field.
- Enabling App Metrics does not grant consent for search collection or a
  search experiment.

There is no search experiment protocol or endpoint in v1. Adding one requires
a separate, default-off consent purpose, an explicit field allowlist, a fixed
retention and deletion schedule, a policy switch independent from App Metrics,
and privacy/security approval before production deployment. Until that new
contract exists, search data cannot leave the device.

## Verification

The daemon test uses a network implementation that panics on every Catalog,
resource, package or metrics operation, then performs a search and verifies
that metrics state and cache contents remain unchanged. Metrics tests inject
forbidden query and experiment fields and require strict decoding to fail.
System Shell tests keep recent queries inside a newly initialized UI struct;
there is no persistence API for that state.
