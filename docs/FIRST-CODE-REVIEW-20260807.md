# CardputerZero-OS Initial Code Review Report

<!-- doc-locale: en -->
> **English** | [简体中文](FIRST-CODE-REVIEW-20260807.zh-CN.md)

- Review date: 2026-08-07 (Asia/Shanghai)
- Review baseline: `main@15e7f29bafc72fbf72e6e3afe4b4289a92f6778a`
- Review type: initial, requirements-driven review of the entire repository
- Conclusion: no P0 findings; four P1, four P2, and two P3 findings confirmed. All four P1
  findings have code fixes. CR-004 received additional direction, color, and speed fixes
  after physical-device testing on 2026-08-07 and still requires validation with a new image.
  P2 and P3 items that require hardware, production infrastructure, or product decisions
  remain open.

## 1. Scope, Basis, and Method

This review covers the Rust workspace, System Shell C code, image and Shell gates, GitHub
Actions, three web consoles, Store services, and their dependencies. It is not a line-by-line
formal audit, and it does not treat an existing implementation as equivalent to hardware or
production acceptance.

Requirements and release boundaries come from these documents:

- `docs/ROADMAP.md`: phase completion criteria and hardware gates;
- `docs/REMAINING-ROADMAP-AUDIT.md`: the boundary between implementation evidence and
  external acceptance;
- `docs/ARCHITECTURE.md` and `docs/THREAT-MODEL.md`: trust boundaries and release blockers;
- `docs/PHASE6E-SECURITY-VALIDATION.md`: the internal security baseline requires
  `make check`, workspace Clippy, and a clean diff;
- `docs/SYSTEM-EXPERIENCE-ROADMAP.md`, `docs/HOME-SYSTEM-APPS-ROADMAP.md`, and
  `docs/STORE-ROADMAP.md`: open conditions for the system experience, system apps, and Store;
- ADRs 0006, 0007, and 0008: accepted decisions for update/rollback, first boot, and the
  early splash.

The review included reading diffs and call paths, inspecting dangerous C parsing paths,
building with pinned toolchains, running the main and web gates, auditing RustSec and npm
dependencies, and cross-checking roadmap claims against implementation status.

Severity definitions: P0 is an issue that can immediately block release or cause a critical
security compromise; P1 is a definite correctness, boot, or continuous-integration gate
defect; P2 is a risk that must be closed or explicitly accepted before release; and P3 is a
maintainability or pre-deployment hardening item.

## 2. Findings and Disposition

### CR-001 [P1 Fixed] Truncated System Shell settings file caused an uninitialized read

**Requirement basis:** Settings must be configured through the trusted Shell and broker.
Invalid or damaged persistent state must fail closed and must not contaminate current
settings.

**Evidence and impact:** When the original `cp0_shell_settings_load()` parsed only some
fields with `fscanf()`, it constructed a candidate structure from uninitialized `theme`,
`timeout`, and `key_sounds` values before checking the parse result. Reading uninitialized
automatic variables is undefined behavior in C. A truncated or power-loss-damaged
`settings.conf` could produce indeterminate settings or an incorrect return value, and the
optimizer could theoretically amplify the behavior.

**Fix:** `system-shell/src/shell_settings.c:39-54` now records the exact number of parsed
fields. It validates `parsed == 4`, `fclose()`, the version, and the Boolean range before
constructing the candidate structure. Failure leaves the caller's output unchanged.
`tests/system-shell-settings.c:24-36` adds a regression case for a truncated file containing
only the version field.

**Verification:** `./tests/test-system-shell-ui.sh` passes, and the full `make check` includes
this test.

### CR-002 [P1 Fixed] Main CI did not execute the repository-defined acceptance boundary

**Requirement basis:** `docs/PHASE6E-SECURITY-VALIDATION.md:46-51` explicitly requires
internal acceptance to run `make check`, workspace Clippy, and a clean diff.
`docs/THREAT-MODEL.md:103-113` maps `make check` to schema, protocol, package, sandbox,
permission, malicious-application, and recovery security checks.

**Evidence and impact:** The original `.github/workflows/ci.yml` checked formatting, two
JSON files, and `cargo test` only. A pull request could regress image profiles, production
access boundaries, compositor/Shell/appd protocols, the SDK ABI, malicious-application
handling, security validation, Store protocols, or any of the three web consoles while CI
remained green.

**Fix:** `.github/workflows/ci.yml:13-40` now installs pinned Rust 1.85.1,
`wasm32-unknown-unknown`, Clippy/rustfmt, and Node 22. It installs all three locked npm
dependency sets and runs the complete `make check`, a blocking `clippy::correctness` pass,
the three web `check` commands, and `git diff --check`. `Makefile:74-78` adds `--locked` to
workspace check/test so validation cannot drift from `Cargo.lock`.

The workflow does not use `-D warnings`. Existing warnings include non-correctness technical
debt in API complexity, readability, and example targets. Promoting all of them to release
blockers would expand this review's scope; this change blocks only the correctness category.

**Verification:** Clippy passes with the pinned toolchain; the complete `make check` and all
three web checks pass.

### CR-003 [P1 Fixed] audiod was incompatible with the declared Rust 1.85 MSRV

**Requirement basis:** `Cargo.toml:53` declares `rust-version = "1.85"`. The DevKit and
documentation pin Rust 1.85.1, and `.github/workflows/devkit.yml:24-27` uses the same version.

**Evidence and impact:** `cp0-audiod` used an `if let ... && let ...` let-chain. It compiled
with a newer local toolchain, but Rust 1.85.1 failed with `E0658`, breaking both pinned CI and
the declared minimum-version build. This also showed that main CI had not previously checked
the MSRV.

**Fix:** `crates/cp0-audiod/src/lib.rs:881-894` now uses two semantically equivalent nested
`if let` expressions. It preserves the original behavior: persistence failure returns
Internal and does not update in-memory state.

**Verification:**
`cargo +1.85.1 clippy --workspace --all-targets --locked -- -D clippy::correctness` passes;
`cargo +1.85.1 test -p cp0-audiod --locked` passes 10 of 10 tests.

### CR-004 [P1 Code Fixed, Hardware Revalidation Pending] Early splash boot, direction, and color errors

**Requirement basis:** ADR 0008 requires splash failure not to block standard initramfs root
discovery or Home boot. V0.6 uses the BCM2837 platform.

**Evidence and impact:** The direct-SPI helper originally used the BCM2835 `0x20000000`
peripheral base, and its RX FIFO drain had no independent iteration bound. Incorrect register
mapping or an anomalous SPI status could hang the early boot path.

**Fix:** Commit `15e7f29` changes
`image/pi-gen/stage-cardputerzero-os/00-bsp/files/early-splash-spi.c` to the BCM2837
`0x3f000000` base and bounds both the helper and RX drain. `early-splash-initramfs` also wraps
the helper with BusyBox `timeout -s KILL 2`. Image-profile tests pin the address, both timeout
layers, and the bounded drain. This part is already in the review baseline and is not an
uncommitted change.

**Verification:** `tests/test-image-profile.sh` and the complete `make check` pass.
`tests/test-built-rootfs-profile.sh`, which requires a finished image, was not run. The next
production-image boot must still verify actual pixels and boot timing on hardware.

**2026-08-07 hardware follow-up:** The boot blocker was closed, but the first direct-SPI
frame was vertically inverted and had incorrect colors; the later DRM framebuffer redraw
was correct. The experimental prototype's `MADCTL=0x60` and minimal initialization sequence
had only been validated with solid colors. The fixed BSP uses the display's actual
`MADCTL=0xa0`, complete power and gamma parameters, and display inversion. The current
uncommitted change in
`image/pi-gen/stage-cardputerzero-os/00-bsp/files/early-splash-spi.c` synchronizes that BSP
configuration and replaces byte-at-a-time RX draining with a bounded TX/RX FIFO stream pump.
`tests/test-image-profile.sh` pins the new direction, color initialization, and transfer
structure. After host gates pass, a newly flashed image must revalidate cold-boot direction,
color, time to first frame, and the DRM takeover transition.

### CR-005 [P2 Hardened] GitHub Actions dependencies used movable tags

**Requirement basis:** Threat Model `SUPPLY-01` requires pinned build inputs. Workflow code
is part of the trust chain for release and fuzz artifacts.

**Evidence and impact:** Three workflows used `actions/checkout@v4`,
`actions/setup-node@v4`, and `actions/upload-artifact@v4`. Upstream can move these tags, so a
build cannot be strictly bound to reviewed action content.

**Fix:** `.github/workflows/ci.yml`, `devkit.yml`, and `fuzz.yml` now pin:

- `actions/checkout@11d5960a326750d5838078e36cf38b85af677262` (v4);
- `actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020` (v4);
- `actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02` (v4).

Version comments remain for Dependabot or manual upgrades. Every future SHA update still
requires review of the release notes.

### CR-006 [P2 Open] Store PostgreSQL vertical acceptance is absent from the default gate

**Requirement basis:** The Store roadmap relies on PostgreSQL 17 vertical tests as evidence
for identity, review, publication, scanning, and Catalog state machines. In-memory core tests
are not a substitute.

**Evidence and impact:** These five integration tests require
`CP0_STORE_TEST_DATABASE_URL`, are `#[ignore]` by default, and do not run under `make check`:

- `crates/cp0-store-control-server/tests/postgres.rs:75`;
- `crates/cp0-store-portal-server/tests/postgres.rs:121`;
- `crates/cp0-store-workforce-server/tests/postgres.rs:122`;
- `crates/cp0-store-scan-worker/tests/postgres.rs:19`;
- `crates/cp0-store-publisher/tests/postgres.rs:30`.

This review therefore confirms that the code compiles, but it cannot claim that this commit
has rerun and passed real database transactions, constraints, and HTTP vertical acceptance.

**Proposed resolution:** Add a separate CI job with a temporary, digest-pinned PostgreSQL 17
service, a random test database, and a non-production `CP0_STORE_TEST_DATABASE_URL`. Run
`make store-control-db-check` and block merging on failure. Production migration still needs
a separate drill. The workflow was not changed because maintainers must first freeze the CI
image digest, resource budget, and run frequency.

### CR-007 [P2 Known Dependency Advisory] `rsa 0.9.10` matches RUSTSEC-2023-0071

**Evidence:** `cargo-audit` scanned 376 dependencies in `Cargo.lock` and reported a Marvin
timing attack with CVSS 5.9 (medium); no fixed version is currently available. The path is
`rsa -> openidconnect -> cp0-store-portal-server -> cp0-store-workforce-server`.

**Exploitability:** Current code uses an RSA public key from OIDC/JWKS to verify JWTs. It
neither holds an RSA private key nor performs RSA private-key decryption. The advisory's key
recovery prerequisite is therefore unreachable in the current service path. This is not a
current P0 or P1, but it must not be silently ignored.

**Resolution:** Track upstream `openidconnect` and `rsa`. When an upgrade is available,
revalidate RS256 through the PostgreSQL/OIDC tests. If any RSA private-key operation is added
later, raise the risk level again before merging. Do not remove the product's required RS256
compatibility merely to clear the scanner result.

### CR-008 [P2 Release Blocker] Critical hardware and production-security evidence remains open

This is not one code bug, but it blocks a claim of production completion:

- Phase 2's 24-hour compositor/Shell/appd stability, memory, and SD-write evidence in
  `docs/ROADMAP.md:60` remains open;
- first boot, Developer Access, system power control, Store S9, Owner USB Media, and
  continuous audio coexisting with key clicks still have V0.6 hardware gates;
- `docs/THREAT-MODEL.md:89-101` explicitly records the absence of trusted verified boot,
  at-rest data encryption, an independent security review, and a production USB VID/PID;
- dm-verity, RAUC A/B, U-Boot/FIT, and the hardware root of trust in ADR 0006 are accepted
  architecture only, without spare-hardware fault-injection and rollback evidence;
- the Store's production HSM/key ceremony, IdP/JWKS, CDN, multi-region recovery, formal
  governance policies, and third-party security/fairness reviews still depend on real
  infrastructure and accountable decisions.

**Resolution:** Keep the roadmap checkboxes open. Use the hardware acceptance scripts to
collect the complete run directory, duration, failure count, reboot/memory/SD-write summary,
and independent-validator output. Product, security, and operations must freeze production
infrastructure before implementation. Host tests from this review must not substitute for
any of this evidence.

### CR-009 [P3 Open] Multitasking is still a model and protocol foundation, not a complete runtime chain

**Evidence:** `docs/MULTITASKING-MERGE-REPORT.md:71-85` states that TaskJournal startup
recovery, Runtime control, real checkpoint callbacks, compositor capture, and the pressure
strategy are not connected. UID authentication alone cannot distinguish a stale surface
after the same app restarts.

**Impact:** The current system cannot claim live thumbnails, reliable device recovery, or a
production CM0 background policy.

**Resolution:** Follow ROADMAP Phase 6L-C through 6L-G: bind
`(task_id, runtime_generation)`, connect journal/reconciliation, add compositor-sealed
thumbnails with a 2 Hz rate limit, integrate WAMR checkpointing with fuel and deadline
limits, and complete hardware acceptance with 1, 3, and 10 apps plus power-loss scenarios.
This review does not implement an unfrozen runtime protocol.

### CR-010 [P3 Open] Web deployment and general static-quality policy are not frozen

All three Vite consoles set `sourcemap: true`. This is useful for engineering diagnostics,
but publicly deploying `.map` files would expose source structure and internal paths. The
repository also lacks production contracts for domains, TLS/CSP, and real IdP/JWKS
deployment. Standard Clippy still reports non-correctness findings such as precedence,
large enum variants, type complexity, `Result<_, ()>`, unnecessary casts, and inefficient
hex formatting. A uniform production policy for `unwrap` and `expect` is also not frozen.

**Resolution:** Once deployment design is fixed, publish sourcemaps only as private
error-tracking artifacts or disable them in production builds, and enforce the decision with
CSP/header integration tests. Create a technical-debt batch to clear Clippy findings by
crate, starting with protocol bit-operation parentheses and service error types before
considering large enum or API changes. This review does not change deployment behavior or
public interfaces.

## 3. Changed Locations

| Change | Location | Status |
| --- | --- | --- |
| Fail closed on a truncated settings file | `system-shell/src/shell_settings.c`, `tests/system-shell-settings.c` | Uncommitted, verified |
| Extend main CI to repository, web, and Clippy gates | `.github/workflows/ci.yml`, `Makefile` | Uncommitted, verified |
| Pin Actions to commit SHAs | `.github/workflows/ci.yml`, `devkit.yml`, `fuzz.yml` | Uncommitted, verified |
| Rust 1.85 audiod compatibility | `crates/cp0-audiod/src/lib.rs` | Uncommitted, verified |
| Bound early-splash failure and use the correct peripheral address | Six files in commit `15e7f29` | Committed; hardware boot blocker closed |
| Early-splash direction, color initialization, and FIFO transfer | `early-splash-spi.c`, `test-image-profile.sh`, ADR 0008, `PHASE1-BSP.md` | Uncommitted; host gate passed, hardware retest pending |
| Initial review archive | `docs/FIRST-CODE-REVIEW-20260807.md` | This document |

## 4. Verification Results

| Check | Result |
| --- | --- |
| `make check` (outside the sandbox, with localhost listening permitted) | PASS |
| Rust 1.85.1 workspace Clippy, `-D clippy::correctness` | PASS |
| `cargo +1.85.1 test -p cp0-audiod --locked` | PASS, 10 tests |
| `./tests/test-system-shell-ui.sh` | PASS |
| Developer Portal `npm test` + production build | PASS |
| Review Console `npm test` + production build | PASS |
| Store Operations `npm test` + production build | PASS |
| Three npm lockfile audits | PASS, 0 vulnerabilities |
| GitHub workflow YAML parse | PASS |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |
| `cargo-audit` | FAIL (one analyzed advisory; see CR-007) |

The complete `make check` ran outside the sandbox because `tests/test-store-origin.sh` and
the `cp0-stored` HTTP Range test must listen on `127.0.0.1`. The managed sandbox rejects the
listener with `EPERM`; that limitation is not a product failure. Both tests passed outside
the sandbox.

## 5. Work Not Executed and Claim Boundary

This review did not run the ignored PostgreSQL 17 tests, long fuzz campaigns, a complete
Docker compositor build, or final rootfs/compressed-image mount checks. No V0.6 hardware,
test Store endpoint, production IdP/JWKS/CDN/HSM, or external power meter was available.
The report therefore proves only the current host-side code and gate status. It is not
production-image, hardware-stability, or production-security acceptance.

## 6. Next Steps

1. Merge the uncommitted P1/P2 fixes recorded here and observe the first successful Ubuntu
   run in the GitHub main CI.
2. Freeze the PostgreSQL 17 CI service digest and budget, then close CR-006.
3. After completing Phase 2's 24-hour evidence, execute first boot, Developer Access, Power,
   Store S9, USB Media, audio, and performance hardware gates in roadmap order.
4. Once the protocol and hardware foundation is stable, advance multitasking from 6L-C
   through 6L-G without prematurely claiming complete multitasking.
5. Before production release, implement the verified-update/boot decision, production
   HSM/IdP/CDN/governance infrastructure, and independent security review. Recheck CR-007's
   upstream status for every release.

This report is the baseline for the initial review. Later findings should preserve their
`CR-NNN` identifier, status transitions, fix commit, and acceptance evidence so traceability
is not lost between roadmap checkboxes and implementation.
