# CardputerZero-OS Agent Collaboration Rules

<!-- doc-locale: en -->
> **English** | [简体中文](AGENTS.zh-CN.md)

This file applies to the entire repository. It defines durable rules for
collaboration, implementation, and validation; it does not record temporary
progress for a particular task. If a subdirectory gains a more specific
`AGENTS.md`, that file applies only within the subdirectory and must not relax
the security, requirement, or acceptance boundaries defined here.

## 1. Sources of Truth

Use the following order when interpreting requirements and deciding whether an
implementation is correct:

1. The user's current explicit requirements, constraints, and latest feedback.
2. Open acceptance criteria in `docs/ROADMAP.md` and related specialized
   roadmaps.
3. Architecture decisions with Accepted status in `docs/adr/`.
4. `docs/ARCHITECTURE.md`, `docs/THREAT-MODEL.md`, and the corresponding phase
   documents.
5. Existing contracts expressed by committed code, schemas, tests, and build
   scripts.
6. Supporting material such as README files, historical reports, and comments.

When these sources conflict, do not select the most convenient interpretation.
First determine whether a source is obsolete, then clearly report the conflict,
impact, and recommendation to the primary agent or the user. Evidence from a
physical device takes precedence over inferences from code that has not been
validated on a physical device, but every observation must still record the
image version, hardware version, and reproduction conditions.

Do not treat the following as completed requirements: unchecked roadmap items,
future approaches in ADRs, test fixtures, mocks, hardware paths that pass only
on the host, or work explicitly marked pending, deferred, or open in the
documentation.

## 2. Synchronization Before Work

Before analysis or editing, every agent must:

- Read this file and any more specific `AGENTS.md` in the target directory.
- Read the roadmaps, ADRs, phase documents, and tests directly related to the
  task.
- Run `git status --short` to identify existing modifications and untracked
  files.
- Run `git diff -- <path>` for every file it plans to edit, comparing the
  working copy with `HEAD`.
- Check for other consumers of the same behavior, including image builds,
  runtimes, services, schemas, tests, and documentation.
- Work from the current workspace contents, not from a stale snapshot captured
  when the task began.

Existing modifications belong to the user or another agent by default. Do not
revert, overwrite, reformat, or opportunistically clean up unrelated changes.
When existing changes overlap the task, understand and build on them; request
coordination only when they cannot be merged safely.

## 3. Multi-Agent Collaboration Protocol

The primary agent is responsible for interpreting requirements, dividing work,
final integration, and reporting the result. A sub-agent handles only its
explicitly assigned scope and must not expand requirements or modify adjacent
modules without authorization.

### 3.1 Division of Work

- Prefer assigning edit ownership by non-overlapping directories or files. A
  shared file may have only one writer at a time.
- Research, code reading, and test analysis may run in parallel; continuous
  edits along one implementation path should be owned by one agent.
- Every assignment must state the objective, requirement basis, paths that may
  be modified, paths that must not be touched, and expected validation.
- A sub-agent that discovers an out-of-scope issue reports its evidence,
  severity, and recommendation without editing it.
- The primary agent must recheck workspace state both when dispatching work and
  when receiving a handoff, so it does not rely on stale information.

### 3.2 State Synchronization

Task state comes from current user messages, agent messages, and the actual
workspace. Do not infer current state solely from a conversation summary, old
diff, previous test result, or this file.

Every handoff must contain the following fields. If a field cannot be completed,
state why:

```text
Objective:
Status: complete / partially complete / blocked
Requirement basis:
Key decisions:
Modified files:
Related files not modified:
Validation performed: command + PASS/FAIL
Validation not performed: reason
Open risks or physical-device retest items:
```

Test results must match the latest files at handoff time. If any edit occurs
after a test, that earlier result must not be reported as the final PASS. After
integrating all work, the primary agent must reread the complete diff and rerun
validation appropriate to the final state.

### 3.3 Conflict Handling

- If a file changes during the work, stop writing it and reread the diff before
  deciding how to merge.
- Do not use `git reset --hard`, `git checkout --`, forced cleanup, or bulk
  overwrites to resolve collaboration conflicts.
- Do not delete files of unknown origin or claim another agent's modifications
  as your own validated work.
- Changes to a shared interface require coordinated compatibility checks across
  producers, consumers, schemas, versions, and test fixtures.

## 4. Requirement-Driven Change Boundaries

Every code change must trace to a user requirement, roadmap acceptance
criterion, accepted ADR, security invariant, or explicitly reproduced defect.
Do not use a task as an opportunity to add features, change public interfaces,
replace dependencies, upgrade toolchains, alter visual design, or perform broad
refactoring.

Prefer the smallest complete behavioral change when fixing a problem:

- Establish the root cause and call chain before editing.
- Preserve existing protocols, error semantics, and fail-closed behavior.
- Add regression tests for confirmed defects; tests should lock down behavior
  or contracts rather than irrelevant implementation details.
- When a requirement is unclear, first check the roadmap and ADRs. Ask the user
  only if the remaining ambiguity affects product behavior.
- Do not substitute "the code exists" for image, physical-device, production
  infrastructure, or long-duration stability acceptance.

Code reviews report findings before proposing changes. Each finding must include
severity, requirement basis, code evidence, impact, proposed change, location,
validation result, and residual risk.

## 5. Project Invariants

Unless the user explicitly changes a requirement and the related ADR or roadmap
is updated, preserve these boundaries:

- The target hardware is CardputerZero V0.6: Raspberry Pi CM0, 512 MB RAM,
  320x170 LCD, and SD card.
- The product memory target is 64 MB VideoCore / 448 MB ARM. Do not reintroduce
  the M5Stack bootscreen firmware that forces a 256/256 MB split. See
  `docs/adr/0003-cm0-memory-and-bsp.md` and ADR 0008.
- BSP, firmware, kernel, Weston, Rust, and Node versions follow the repository's
  existing pinning policy. An upgrade is a separate requirement and must state
  its compatibility and supply-chain impact.
- The product splash uses the user-provided image pinned and hash-verified in
  the repository. Orientation, color, DRM takeover, and boot timing follow
  `docs/adr/0008-early-boot-splash.md`.
- Third-party apps may use system capabilities only through the SDK/WASM layer
  and capability brokers. They must not gain ambient authority over Linux
  devices, paths, arbitrary IPC, or another app's data.
- Authentication, size limits, timeouts, atomic writes, and fail-closed
  constraints in System Shell, the compositor, appd, brokers, Store, and the
  update/recovery path must not be weakened for implementation convenience.
- Keep console, SSH, writable-root, and service boundaries separate between the
  production and recovery images.
- Do not commit passwords, tokens, private keys, production endpoints, real
  user data, or absolute paths that apply only to one local machine.

## 6. Editing Conventions

- Follow the target file's existing language, format, and naming style. Use
  ASCII by default. Maintained Markdown documentation follows
  `docs/LOCALIZATION.md`: English is the default `FILE.md`, and Simplified
  Chinese is the paired `FILE.zh-CN.md`.
- Keep changes focused. Do not run repository-wide automated fixes or
  formatting that touches unrelated files.
- Use an appropriate parser or serializer for structured data instead of
  fragile string concatenation.
- Use strict quoting and bounded waits in shell scripts. In C/C++, account for
  integer ranges, lengths, initialization, resource release, and error paths.
  Keep Rust compatible with the workspace MSRV and `Cargo.lock`.
- Comments should explain non-obvious constraints or reasons, not restate code.
- Update generated artifacts, binary firmware, or splash assets only when the
  requirement explicitly calls for it; update their source, hash, generation
  method, and image gate at the same time.

## 7. Validation Strategy

Validation scope grows with risk. Run the smallest relevant module test first,
then consider the complete gate.

Common entry points:

```sh
make check
make portal-check
make review-console-check
make store-operations-check
make verify-image
```

Also select validation appropriate to the change:

- Rust: `cargo test -p <crate> --locked`, and Clippy with pinned Rust 1.85.1
  when needed.
- C/System Shell: the corresponding `tests/test-*.sh`, using the existing
  compiler flags to check warnings.
- Image/BSP: `tests/test-image-profile.sh`, the relevant rootfs profile, and an
  actual image build.
- Web: lockfile installation, tests, type checking, and a production build for
  the target project.
- Schemas/protocols: success, boundary, unknown-field, truncation, replay, and
  version-mismatch tests.
- Security boundaries: failure paths, permission denial, symlinks, size/time
  limits, and atomic recovery.

Some Store tests in `make check` must listen on `127.0.0.1`; a restricted
sandbox may return `EPERM`. Clearly distinguish environmental restrictions from
product failures and rerun the tests in an approved environment. Do not report a
skip or unit-test substitute as a complete PASS.

The following validations are not interchangeable:

- Host static gates do not replace testing on a V0.6 physical device.
- Unit tests do not replace inspection of the finished rootfs/image.
- Short tests do not replace the roadmap's 24-hour stability or power-loss
  tests.
- Mock or in-memory Store tests do not replace production paths such as
  PostgreSQL, OIDC, HSM, and CDN.
- One successful boot does not close independent gates for orientation, color,
  memory, restart, power-off, and recovery.

## 8. Documentation and Completion Criteria

When an implementation changes product behavior, an architecture decision,
build input, or acceptance method, update the nearest authoritative document in
both languages and update its tests. Do not duplicate mutable state across documents: roadmaps record
completion criteria, ADRs record durable decisions, phase documents record
implementation and acceptance methods, and dated reports record a particular
review or physical-device evidence.

A task is complete only when:

- The implementation matches the explicit requirement without changing
  unrelated behavior.
- The final diff has been reread and does not overwrite pre-existing workspace
  changes.
- Relevant tests have run against the final files and their results are
  recorded.
- Unexecuted items, environmental restrictions, and residual physical-device or
  production risks are stated clearly.
- No known conflict remains among documentation, schemas, tests, and
  implementation.
- The handoff gives exact file locations so the next agent can continue without
  guessing.

Committing, pushing, publishing an image, modifying external services, or
closing hardware acceptance items all require explicit user authorization. Code
and host gates passing do not constitute such authorization.
