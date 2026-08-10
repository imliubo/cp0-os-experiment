# Store Policy v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-POLICY-V1.zh-CN.md)

This document is the engineering policy baseline for review and operations in the first
CardputerZero Store release, which supports free apps only. It defines minimum enforceable
rules and structured reason codes, but does not replace developer terms, privacy policy, or
legal advice for a target market. Product, security, and legal owners must approve the final
production policy before public launch.

## 1. Content and Behavior

An app must not contain or facilitate:

- malware, credential theft, undeclared download-and-execute behavior, sandbox escape,
  permission bypass, or interference with trusted System Shell;
- deceptive identity, imitation system permission prompts, forged install/security state,
  or concealment of core behavior;
- unauthorized collection, sale, or correlation of personal data, device fingerprinting,
  or disclosure of sensitive data through URLs or logs;
- illegal content, explicit instructions for physical harm, harassment or hate, child sexual
  exploitation, or unlicensed intellectual property;
- control of GPIO, radio, or peripherals that could cause physical or property damage without
  safety bounds and a clear risk explanation;
- bypass of regional frequency, transmit-power, export-control, or other hardware-compliance
  limits;
- payments, subscriptions, gambling, real-money transactions, advertising attribution, or
  crypto-asset custody, none of which the first release supports.

An app must perform its core function under the offline/network conditions described in its
Listing. Empty shells, website shortcuts, debug probes, and permission-test-only packages
must not be published as ordinary user apps. Development and acceptance tools may use an
explicit private distribution channel and do not appear in public rankings.

## 2. Privacy

- Every Listing provides a publicly accessible HTTPS privacy URL, including when the app
  declares that it collects no data.
- Every manifest permission has a user-comprehensible reason consistent with actual behavior;
  unused permissions are removed.
- `network.client` does not authorize transmission of data obtained through another
  capability. Review covers data type, purpose, recipient, retention, and deletion.
- Keyboard input, documents, microphone, camera, GPIO, LoRa data, and stable device
  identifiers must not be collected for cross-app or cross-service tracking unless required
  for a user-requested function and separately approved by policy.
- App-private data remains on the device by default. External transfer is minimized, uses
  HTTPS, and is accurately disclosed in the privacy statement.
- Store search terms are not uploaded. Store telemetry is disabled by default in the first
  release; future aggregate metrics require separate consent, de-identification, and a fixed
  retention period.
- Support or review materials must not require real user data. Reproduction data is synthetic
  or explicitly redacted.

A change to the privacy statement, manifest permissions, or actual network behavior creates
a new Submission revision and requires another review.

## 3. Age Rating

The developer declares `4+`, `9+`, `12+`, or `17+`. Reviewers raise the rating as required by
violence, fear, mature themes, user-generated content, external web pages, or uncontrolled
communication. Descriptions and screenshots must be appropriate for the displayed rating.
The first release has no parental account or purchase controls, so device settings cannot
weaken this policy.

## 4. Review Process

Automated scanning runs without network access and with bounded CPU, RAM, and time. It checks:

- `.capp` format, developer signature, manifest, WASM/AOT, imports, and permission binding;
- Listing schema, localization, resource digests, complete PNG decoding, dimensions, and
  malicious fixtures;
- permission, import, network-target, and binary differences from the previous published
  version;
- known malicious digests, leaked secrets/credentials, abnormal compression, and parser
  differentials.

Manual review verifies that the Listing matches behavior, the keyboard workflow can be
completed, no critical content is obscured at 320x170, permissions trigger at the right time,
and the privacy statement is accurate. `audio.capture`, `camera.capture`, `hardware.gpio`,
`radio.lora`, and security exceptions require approval from a second reviewer.

Review decisions use stable reason codes including at least:

```text
content-illegal              metadata-misleading
malware-or-evasion           functionality-incomplete
privacy-undisclosed          permission-unnecessary
permission-reason-mismatch   hardware-safety
regional-radio-policy        intellectual-property
age-rating-incorrect         asset-invalid
package-invalid              duplicate-or-spam
```

`needs-changes` and `rejected` include field-level reasons and actionable guidance. Security
detection detail may be limited to avoid helping evasion. Developer replies are append-only;
changing any submitted object requires a new revision. Reviewers cannot directly edit
developer copy, resources, or packages.

## 5. Publication, Pause, and Removal

Only an `APPROVED` Submission can create a Release. Editorial placement can reference only a
releasable Release and cannot bypass review. A Release may be paused or removed for:

- developer withdrawal;
- severe crashes, data corruption, or incompatibility with the current OS;
- privacy, content, intellectual-property, or regional-compliance issues;
- developer-key compromise, malicious behavior, or an actively exploited vulnerability;
- a legal order or trusted emergency-security response.

Ordinary pause/removal stops new discovery, installation, and update through a
higher-sequence Catalog. It neither overwrites nor deletes immutable publication objects.
The Store does not silently remove installed apps or their private data. If an installed
version presents an immediate risk, the security team uses separately signed device policy
or an OS update to block launch, retains audit evidence, and clearly explains the impact and
data-recovery path to users.

An emergency action records at least the actor, dual approval or emergency exception, reason,
object digest, affected region, Catalog sequence, and notification status. Restoring
visibility also requires a higher sequence; an old Catalog cannot be reused or rolled back.

## 6. Appeals and Transparency

A developer may file one structured appeal of a review or removal. A reviewer who did not
make the original decision handles it. An appeal does not automatically suspend emergency
security action before the risk is resolved. Every decision, reply, escalation, and policy
exception remains on an immutable timeline.

The Store periodically publishes aggregate transparency data: submission volume, decisions
by reason code, appeal outcomes, emergency removals, and average handling time. It does not
publish developer secrets, vulnerability details, or user data. Review fairness and false
positive rates receive an independent retrospective.

## 7. Production Gates

Before public launch, complete: final policy/privacy/developer terms and a change-notification
mechanism; reviewer training and access audits; on-call procedures for reports, intellectual
property, child safety, law-enforcement requests, and security incidents; regional
applicability review; data retention/deletion jobs; and a drill covering removal, appeal,
Catalog withdrawal, and blocking an already installed malicious version.
