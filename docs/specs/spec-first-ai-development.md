# Spec-first AI development

Status: active  
Date: 2026-09-21  
Repository: `bhrumom/fabushi-cli`  
Repository scope: standalone CLI, TUI, harness, CLI-specific runtime adapters, and CLI release tooling

## Rule

**No Spec, No Code.** Every AI/developer task that changes product behavior must read a durable applicable Spec first. If none exists, create one under `docs/specs/<task-name>.md` from `docs/specs/SPEC_TEMPLATE.md` before implementation.

Read-only discovery may precede the Spec only to understand the current state and write/repair the Spec.

## Repository identity

This repository is authoritative only for the scope stated above. If a task belongs to another Fabushi split repository, switch repositories before implementation. Legacy product copies in `bhrumom/fabushi` are migration/reference material, not an alternate implementation location.

## Minimum Spec

A usable Spec defines:
1. context/problem;
2. goal;
3. non-goals/out of scope;
4. requirements (stable IDs for non-trivial work);
5. verified current state;
6. target state;
7. architecture/ownership boundaries;
8. interfaces/contracts/schemas/data flow when applicable;
9. relevant non-functional constraints;
10. failure modes/edge cases;
11. implementation strategy;
12. verification/test strategy;
13. acceptance criteria/Definition of Done;
14. release/migration/rollback/observability when applicable;
15. references/provenance.

## Lifecycle

1. **Discover** current code, repository identity and authoritative sources.
2. **Spec** — read, create or repair the durable Spec.
3. **Architecture/Plan** — derive ownership/interfaces/tasks/risks from it.
4. **Implement** within declared scope.
5. **Verify** against the exact requirements and current source revision.
6. **Spec Compliance Review** — mark each requirement/AC `passed`, `blocked`, or `not-applicable`, with evidence/reason.
7. **Integrate/Deliver** through applicable PR/merge/release gates.

The latest explicit user requirement may amend an older Spec, but the durable Spec/decision record must be updated so repository state does not intentionally diverge from documented requirements.

## Acceptance

This policy is effective when the repository root `AGENTS.md`, this file, and `SPEC_TEMPLATE.md` exist on canonical `main`.
