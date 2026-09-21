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

## Mandatory lifecycle

**Discover → Spec → Architecture/Plan → Implement → Verify → Spec Compliance Review → Integrate/Deliver**

- **Discover:** inspect current code/repository facts and locate the owning Spec.
- **Spec:** define goal, non-goals, requirements, target behavior, architecture boundaries, failure cases, verification and Definition of Done.
- **Architecture/Plan:** derive implementation tasks/interfaces/migrations/risks from the Spec.
- **Implement:** code against the durable Spec, not chat memory.
- **Verify:** execute or inspect the verification required by the Spec and latest explicit user instruction.
- **Spec Compliance Review:** compare the final implementation against every requirement/acceptance criterion; record `passed`, `blocked`, or `not-applicable` with evidence/reason.
- **Integrate/Deliver:** follow this repository's PR/merge/release rules. Code written or pushed alone is not completion.

## Fail-closed rules

Do not:
- start product-affecting implementation without reading a usable Spec;
- use chat memory as the only persistent requirement source for substantive work;
- silently expand/reduce scope;
- weaken acceptance criteria to make checks pass;
- leave the Spec stale after intentional design/behavior changes;
- claim completion without requirement-to-evidence review.

Canonical Spec-first policy: `docs/specs/spec-first-ai-development.md`.
