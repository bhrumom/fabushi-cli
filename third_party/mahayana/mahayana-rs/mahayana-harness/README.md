# Mahayana Harness

`mahayana-harness` is Fabushi's Rust-native composable agent harness. It is a clean Rust implementation of the capability-oriented architecture needed by the product; it does not embed Node.js, Python, Cordis, or a second application runtime.

The design is informed by the public, MIT-licensed DeepSeek Harness project and is adapted to Mahayana's existing Rust Runtime/Host/ToolHost/PluginRuntime architecture. The product source of truth remains Mahayana.

## Design rules

1. **One Rust runtime.** Electron, Swift/Kotlin, CLI and headless surfaces share the same Rust services.
2. **Everything replaceable through seams.** Model, storage, tools, plugins and policy are interfaces/registries rather than hard-coded providers.
3. **Model-visible state is replayable.** Session-visible messages and tool lifecycle events are append-only session events; transcript/model history is derived from that log.
4. **Tool execution is policy gated.** Tool registration is separate from execution. Approval and pre/post interceptors run before the platform `ToolHost` boundary.
5. **Plugins do not own the product.** Plugins contribute services/tools/configuration and can be mounted/unmounted without replacing Mahayana Runtime.
6. **Cross-platform capability truth comes from Rust.** Unsupported native capabilities are rejected by `mahayana-tool-host`; they are never silently proxied to a cloud agent.

## DeepSeek Harness capability mapping

This table is an implementation acceptance checklist, not a marketing feature list. A row is considered complete only when the Mahayana Rust path has an implementation and CI coverage.

| DeepSeek Harness area | Mahayana Rust landing point | Current integration |
| --- | --- | --- |
| Core agent/service registry | `mahayana-harness` + `mahayana-agent` | Native registry and agent descriptors implemented |
| Agent loop lifecycle events | `mahayana-harness` session/agent events + existing `mahayana-agent` backend | Event vocabulary and persistent log foundation implemented; backend event mirroring remains to be wired |
| Append-only session log / transcript / fork | `mahayana-harness` | Implemented |
| System/model-visible context | session event projection + existing Mahayana/Codex prompt path | Projection primitive implemented; full prompt section registry pending |
| Tool registry | `mahayana-harness` + `mahayana-tool-host` | Implemented |
| Tool pre/post interception | `ToolInterceptor` | Implemented |
| Tool approvals / interaction | harness approvals + existing FeatureHost approvals | Core implemented; product approval UI bridge pending |
| LLM adapter seam / streaming | `LlmProvider` / `LlmStreamSink` + existing Codex/Responses backends | Seam implemented; provider adapters pending |
| Profiles / bundles / overlays | `ProfileDefinition` | Core profile registration/activation/dump implemented; filesystem overlay loader pending |
| Plugin mount/unmount | `PluginManifest` + `mahayana-plugin-runtime` | Harness lifecycle implemented; artifact-installed plugin bridge pending |
| Subprocess / shell | existing Codex local execution + `mahayana-tool-host` | Existing product capability; harness-native provider registration pending |
| Persistent PTY | existing Codex runtime / platform process host | Provider bridge pending |
| Filesystem tools | existing local ToolHost/Codex | Provider bridge pending |
| Sandbox policy | existing platform policy/computer boundary | Unified harness sandbox seam pending |
| LSP | existing embedded Codex tooling | Harness tool registration pending |
| Code runtime | existing Mahayana JS runtime/Codex execution | Rust service seam pending |
| Skills | existing FeatureHost skill state/plugin system | Harness registry bridge pending |
| Compaction | existing model/runtime backend | Explicit harness compaction service pending |
| Subagents | existing FeatureHost subagents + harness agent registry | State primitives exist; resumable provider bridge pending |
| Agent Teams | existing FeatureHost groups/peer messages | Persistent team seam pending |
| Jobs/background tasks | `JobRecord` + existing FeatureHost async tasks | Harness lifecycle implemented; executor bridge pending |
| Goals | `GoalRecord` | Implemented |
| Workflow / Ralph | `WorkflowRecord` + existing FeatureHost workflows | Registry implemented; worker executor bridge pending |
| Web search/fetch | existing connectors/MCP/Codex | Harness provider bridge pending |
| Attachments/content addressing | `content_address()` + existing app attachments | Hash primitive implemented; content-addressed store pending |
| Spill/result offloading | existing storage paths | Dedicated spill policy pending |
| Todo | existing task/agent surfaces | Dedicated harness todo state pending |
| Plan mode | existing agent/product plan behavior | Dedicated plan state/review transition pending |
| Presets | agent `preset` + profile system | Core field implemented; preset file loader pending |
| Guard/repetition/deadline policy | tool interceptors | Interceptor seam implemented; default guards pending |
| Runtime extension/self-modification | plugin runtime + mount/unmount | Safe plugin lifecycle exists; model-authored extension policy pending |
| Hooks / Codex wire integration | existing embedded Codex crates | Existing product capability; harness event bridge pending |
| Session persistence JSONL/SQLite | existing product persistence + harness session model | Storage provider seam exists; concrete harness stores pending |
| Session query / FTS | existing product search primitives | Harness query service pending |
| Settings | existing `ProductHostSettings` | Harness settings seam pending |
| Credentials | existing Rust-owned secure product session | Existing secure boundary; generic credential-reference seam pending |
| Workspace | `RuntimeConfig.workspace_roots` + context | Workspace entity/service pending |
| SDK / JSON-RPC | existing Mahayana Host JSON ABI | Harness commands in canonical ABI pending |
| ACP | existing agent/host transports | ACP server bridge pending |
| Schedule | existing automations | Harness schedule seam pending |
| Feedback | existing product feedback path | Harness feedback seam pending |
| Identity | existing product account/anonymous runtime state | Shared identity seam pending |
| Remote/E2B providers | provider abstraction only | Optional provider adapters pending; local-first remains default |
| Web/headless UI surfaces | existing Electron/native/Web/CLI hosts | Harness state bridge exported from FeatureHost; UI wiring pending |

## Implemented core API

`MahayanaHarness` currently provides:

- service registration/unregistration;
- profile registration/activation and config dump;
- plugin mount/unmount;
- tool registration, approval gating and pre/post interceptors;
- LLM and storage provider seams;
- append-only session events, transcript derivation and session fork;
- conversation-to-session mapping;
- agent creation;
- goals;
- jobs;
- workflows;
- event polling and runtime snapshots;
- SHA-256 content addressing.

`HarnessFeatureController` in `mahayana-feature-host` is the product-facing bridge. New UI/CLI APIs should go through that bridge or the canonical Mahayana Host ABI rather than importing harness internals directly.

## Verification

The repository forbids local application/Rust builds on the development machine because of disk pressure. Compilation and tests for this crate must run in GitHub Actions. CI acceptance for this migration must include at minimum:

- `cargo test -p mahayana-harness`;
- `cargo test -p mahayana-feature-host`;
- existing Mahayana runtime/host tests;
- formatting/lint checks already enforced by repository CI;
- no Node/Python runtime dependency added to Mahayana Harness;
- desktop/mobile feature graph still resolves under the existing CI workflows.

## Upstream reference

DeepSeek Harness: `deepseek-ai/deepseek-harness` (MIT). Its public architecture and behavior are used as a reference. Mahayana's implementation is written in Rust against Fabushi's existing runtime contracts rather than vendoring the upstream TypeScript application.
