# Fabushi CLI — Agent Instructions

These instructions apply repository-wide to AI-assisted development in `bhrumom/fabushi-cli`.

## CRITICAL: Repository ownership

This repository is the canonical source for **standalone CLI, TUI, harness, CLI-specific runtime adapters, and CLI release tooling**.

- Verify the current GitHub repository before product-affecting work.
- Do not implement another Fabushi platform's product code here. Switch to that platform's canonical repository first.
- `bhrumom/fabushi` is the legacy migration/source-history repository, not the canonical implementation repository for this scope.
- Cross-platform shared runtime/contracts belong in `bhrumom/fabushi-platform-core`; platform-specific consumers remain in their own repositories.

## CRITICAL: Spec-first development — No Spec, No Code

Before changing application/runtime code, tests, schemas, contracts, dependencies, build/release configuration, migrations, security controls, or other behavior-affecting files:

1. Read this `AGENTS.md`.
2. Find and read the applicable durable Spec/project/source-of-truth documents.
3. Check `docs/specs/` for a task/feature Spec.
4. Validate the Spec against the latest explicit user requirement and current repository/GitHub facts.
5. If no usable Spec exists, or it is stale/unclear/contradictory, create or repair the Spec **before implementation** using `docs/specs/SPEC_TEMPLATE.md`.

Read-only investigation needed to understand the system or write the Spec is allowed first. Product-affecting implementation is not.

A test file named `*.spec.*` is not automatically the durable product/development Spec required by this rule.

## CRITICAL: Standard end-to-end development lifecycle

All AI-assisted development must follow this lifecycle unless a newer explicit user instruction or an applicable repository/project specification defines a stricter or more task-specific requirement:

**Discover → Confirm Goal → Spec → Current-State Verification → Architecture → Task Decomposition → Test Design → Implement → Layered Verification → Failure/Recovery Verification → Exact-HEAD CI → Packaged Acceptance → Independent Acceptance → Spec Compliance Review → Protected Integration → Canonical-Main Verification → Release → Post-Release Smoke Test → Evidence Archive → COMPLETE**

The lifecycle is fail-closed: a later stage is not successful when a required earlier gate is incomplete, failed, stale, or unsupported by evidence. A stage may be marked not applicable only with a recorded reason.

### Stage 0 — Discover
- Read the repository and nested agent instructions.
- Locate the owning source of truth, Spec, task, decisions, contracts, and relevant prior evidence.
- Inspect the current code and live GitHub state needed to understand the task.
- Do not treat chat memory, an old branch, an old work-session summary, or a previous release as the current repository state.

### Stage 1 — Confirm Goal
- Record the current observable problem or requested change.
- Record the target externally observable outcome.
- Define scope and non-goals.
- Identify whether architecture, protocols, schemas, UI/UX, security, performance, migration, packaging, or release are affected.

### Stage 2 — Spec
- Read the applicable durable Spec completely.
- If it is missing, stale, incomplete, or contradictory, create or repair it before implementation.
- Define requirements, edge cases, test strategy, acceptance criteria, and Definition of Done before coding.
- Use stable requirement/acceptance IDs for non-trivial work.
- The Spec records durable truth: required behavior, architecture, decisions, constraints, and final acceptance state. It is not a minute-by-minute development log.

### Stage 3 — Current-State Verification
Verify the live facts that materially affect the work, including when applicable:
- canonical/default branch and exact source SHA;
- active branch/PR and exact head SHA;
- actual code/module ownership and dependency structure;
- protocol/dependency versions;
- open conflicting work;
- relevant CI/workflow state;
- current application/release version and published artifacts.

Do not implement against an unverified repository shape.

### Stage 4 — Architecture
- Derive the design from the Spec.
- Define ownership boundaries, state ownership, process/runtime boundaries, dependency direction, interfaces, data/control flow, cancellation, timeout, retry, reconnect/resync, crash settlement, migration, and observability as applicable.
- Record important intentional architecture decisions durably before or together with implementation.

### Stage 5 — Task Decomposition
- Split non-trivial work into small, traceable tasks with independently verifiable outcomes.
- Map implementation tasks back to requirement/acceptance IDs.
- Avoid unrelated architecture, feature, migration, and release changes in one unstructured batch.

### Stage 6 — Test Design
Before implementation, define how each requirement will be proven.

Use the applicable layers:
**Static/Architecture → Unit → Contract → Integration → E2E → Regression → Failure/Recovery → Packaged App → Release/Update Acceptance**

Behavioral test scope must follow the latest explicit user instruction and applicable Spec. Do not invent a behavioral release gate that the user explicitly waived, and do not skip a behavioral gate the user or Spec explicitly requires.

### Stage 7 — Implement
- Implement against the durable Spec and verified current source.
- Stay within scope and preserve architecture boundaries.
- Do not weaken requirements or acceptance criteria merely to make checks pass.
- If intended behavior/design changes during implementation, update the Spec/decision record before or together with the code.
- Do not leave intentional behavior undocumented.

### Stage 8 — Layered Verification
- Run the required verification from the cheapest/narrowest useful layer toward broader layers.
- Diagnose failures at the lowest layer that can explain them.
- Verify affected regression paths, not only the new happy path.
- A source-level test pass is not the same as an application/package pass.
- Any behavioral layer explicitly waived by the latest user instruction must be recorded as not run/waived rather than reported as passed.

### Stage 9 — Failure and Recovery Verification
When applicable, verify abnormal paths such as:
- network interruption/recovery;
- timeout/cancel;
- process/host/runner crash and restart;
- application restart;
- reconnect/resync;
- stale or duplicate events;
- partial response/failure;
- authentication/OAuth failure;
- MCP/tool failure;
- migration interruption/rollback.

### Stage 10 — Exact-HEAD CI
- Bind authoritative CI evidence to the exact source revision being accepted.
- Record PR/branch head SHA and workflow run ID/URL.
- Evidence from an earlier SHA is stale after the head changes.
- When repository policy requires GitHub Actions or another designated build runner, local results are supplementary only.

### Stage 11 — Packaged Acceptance
When the deliverable is installable/deployable and packaged acceptance is required by the Spec or latest user instruction, verify the actual produced artifact, including as applicable:
- build/package;
- embedded binaries/resources;
- signing;
- notarization/stapling;
- updater metadata;
- checksums;
- installation/launch;
- required critical user flows.

A passing source build is not proof that the distributed package works.

### Stage 12 — Independent Acceptance
For non-trivial, release-bound, architecture-sensitive, or high-risk work, use an acceptance pass independent from the implementation reasoning.

Compare:
**Spec ↔ Final Diff/Code ↔ Exact-Source CI ↔ Required Packaged Behavior ↔ Evidence**

Do not accept an implementation session's completion claim as proof.

### Stage 13 — Spec Compliance Review
Before declaring completion, map every applicable requirement and acceptance criterion to:
- `passed` with evidence;
- `blocked` with reason;
- `not-applicable` with reason.

Intentional design/behavior divergence must already be reflected in the durable Spec or decision record.

### Stage 14 — Protected Integration
Typical order:

```text
development branch
→ PR
→ exact-HEAD required CI
→ required packaged acceptance
→ review / independent acceptance
→ Spec compliance
→ protected merge
→ canonical main
```

Do not merge merely because code was written, pushed, or partially tested.

### Stage 15 — Canonical-Main Verification
- Treat the canonical-main merge SHA as the new integrated source identity.
- Do not assume PR-head evidence automatically proves the merged revision.
- Verify post-merge build/release workflows bind to the exact canonical-main SHA when applicable.

### Stage 16 — Release
Release only from the accepted canonical source revision and satisfy applicable release-construction/platform gates, including version correctness, build/package, signing/notarization, metadata, checksums, artifact/store publication, and rollback readiness.

A PR merge is not a release, and a candidate artifact is not a completed release.

### Stage 17 — Post-Release Smoke Test
When post-release behavioral validation is required by the latest user instruction or applicable Spec, obtain the artifact from the real distribution channel and exercise the required critical path. If the user explicitly waived behavioral testing, record the waiver truthfully instead of fabricating a pass.

### Stage 18 — Evidence Archive
Preserve, as applicable:
- final canonical source SHA;
- PR/merge reference;
- workflow run IDs/URLs;
- release version/tag;
- artifact identifiers/checksums;
- test reports;
- screenshots/video;
- logs/traces;
- migration/rollback result;
- known limitations/deferred work.

Detailed action-by-action history belongs in commits, PRs, task/work logs, and CI. Keep the Spec focused on durable requirements, architecture, decisions, and final compliance/evidence references.

### Stage 19 — COMPLETE Gate
A task may be marked `COMPLETE` only when all applicable requirements and required delivery gates are satisfied and evidence-backed.

Repository-wide defaults:

> **No Spec, No Code.**
>
> **No Evidence, No Complete.**
>
> **No required exact-source verification, No Acceptance.**
>
> **No required canonical-main packaged/release verification, No Release Complete.**

Partial implementation, a green unit suite, a successful PR build, a merged PR, or an uploaded candidate artifact is not by itself sufficient to claim end-to-end completion.


## Fail-closed rules

Do not:
- start product-affecting implementation without reading a usable Spec;
- use chat memory as the only persistent requirement source for substantive work;
- silently expand/reduce scope;
- weaken acceptance criteria to make checks pass;
- leave the Spec stale after intentional design/behavior changes;
- claim completion without requirement-to-evidence review.

Canonical Spec-first policy: `docs/specs/spec-first-ai-development.md`.
