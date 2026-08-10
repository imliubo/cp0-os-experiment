# Store Moderation v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-MODERATION-V1.zh-CN.md)

Status: engineering preview; production enforcement is disabled.

This contract defines the privacy boundary and transactional state machines for
content reports, developer notices, appeals, and security-response deadlines. It
does not authorize automatic takedown. Product, legal, safety, and security
owners must approve the policy vocabulary, deadlines, retention, appeal window,
and two-person enforcement procedure before production ingress is enabled.

## Privacy boundary

`POST /reports/v1/content` accepts exactly:

- `release_id`, `app_id`, and exact semantic `version`;
- one fixed reason: `malware`, `privacy`, `fraud`, `harmful-content`,
  `age-rating`, or `other`.

The service rejects unknown fields and accepts no free text, contact details,
account identifier, device identifier, event timestamp, IP address, User-Agent,
log, screenshot, or attachment. The application does not read or persist
network headers other than `Content-Type` and a random `Idempotency-Key` whose
SHA-256 digest expires after 24 hours. Production ingress must not add client IP
or fingerprint data to application logs or forwarded headers.

Reports are accepted only for a release whose App ID and version match an
approved Submission and an immutable published Store package artifact. The
response is `202 Accepted` and exposes only a random report ID, state, SLA class,
deadlines, and resource version. Exact retry returns the same report; reuse of
the key with another body fails.

## Provisional SLA

These constants exist to make queue ordering and overdue behavior testable. They
are not a production commitment:

| Class | Reasons | Acknowledge | Resolve or escalate |
| --- | --- | ---: | ---: |
| `security` | `malware`, `privacy` | 4 hours | 72 hours |
| `standard` | all other reasons | 72 hours | 14 days |

Untriaged reports are ordered by acknowledgement deadline and stable report ID.
The database stores server receipt time only. At most 10,000 untriaged reports are
accepted by this reference service; external rate limiting must operate without
persistent user fingerprinting.

## Roles and transitions

Moderation writes require a live operator token with the exact
`store.moderation` scope, active `admin` role, and 2FA. Developer reads and
appeals re-read live Team ownership, role, token scope, revocation, and 2FA in a
`SERIALIZABLE` transaction.

```text
report: submitted -> closed-no-action
                  -> notice-issued -> closed-after-appeal
                  -> security-escalated

notice: open -> appealed -> resolved-accepted | resolved-upheld
appeal: pending -> accepted | upheld
```

An operator decision contains only a fixed disposition and one to four fixed
reason codes. `developer-notice` creates an immutable, Team-scoped notice.
`security-escalation` emits an outbox request for a separately operated incident
system. Neither transition changes Release state or Catalog publication.

A Team owner or developer can create one appeal within the provisional 14-day
window using one fixed ground: `identity-mismatch`, `policy-misapplied`,
`remediated`, or `other`. An independent operator decision resolves the appeal.
Every write has a strong ETag, exact idempotent replay, append-only revision,
audit event, and outbox event in one transaction.

## API surface

- `POST /reports/v1/content` - privacy-minimized public intake;
- `GET /v1/moderation/reports` - bounded operator SLA queue;
- `POST /v1/moderation/reports/{report_id}:decide` - structured triage;
- `GET /v1/apps/{app_id}/moderation-notices` - Team-scoped notices;
- `POST /v1/moderation/notices/{notice_id}:appeal` - one developer appeal;
- `POST /v1/moderation/appeals/{appeal_id}:decide` - operator resolution.

Production completion still requires approved policy text, abuse controls,
retention erasure, notification delivery ownership, security on-call
integration, two-person enforcement, reversible Catalog suppression, and drills
covering appeal reversal and emergency removal.
