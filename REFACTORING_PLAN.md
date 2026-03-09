# Refactoring Plan

## Phase A Execution Status

- [x] Task 1. Extract shared kernel snapshot builder DONE
- [x] Task 2. Rewire auto and manual checkpoint flows DONE
- [x] Task 3. Validate checkpoint refactor DONE
- [x] Task 4. Extract tools policy module DONE
- [x] Task 5. Extract tools path module DONE
- [x] Task 6. Extract tools runner and audit modules DONE
- [x] Task 7. Validate tools facade refactor DONE
- [x] Task 8. Create shared GUI response parser DONE
- [x] Task 9. Wire models and processes widgets to parser DONE
- [x] Task 10. Wire memory and chat parsing to shared parser where applicable DONE
- [x] Task 11. Validate GUI parser refactor DONE
- [x] Task 12. Create shared Python runtime config loader DONE
- [x] Task 13. Migrate Python entrypoints to shared loader DONE
- [x] Task 14. Run final validation and close Phase A DONE

## Scope

This document maps the current technical debt in AgenticOS after the Qwen3.5 integration stabilized. The goal is not to implement the refactors yet, but to identify where the codebase should be split, simplified, or reorganized, and why.

The assessment covers the Rust kernel under `src/`, the PySide GUI under `gui/`, and the utility scripts currently shipped with the repository.

## Highest Priority

### 1. Split `src/backend.rs` DONE

Current state:
- `src/backend.rs` mixes backend registry, backend resolution, local Candle inference backends, external `llama.cpp` RPC transport, HTTP parsing, diagnostics, slot persistence, and backend tests.
- The file is large enough that unrelated changes collide frequently.

Why refactor:
- It violates separation of concerns. Backend semantics and HTTP transport are different layers.
- The external backend is now materially more complex because of stop semantics, chunking, prompt reconstruction, and reasoning handling.
- Tests for local backends and remote RPC behavior are forced to live in the same file.

Recommended split:
- `src/backend/mod.rs`: traits, shared types, backend loader entry points.
- `src/backend/local.rs`: `QuantizedLlamaBackend`, `QuantizedQwen2Backend`, local-step inference.
- `src/backend/external_llamacpp.rs`: `ExternalLlamaCppBackend`, completion parsing, slot RPC.
- `src/backend/http.rs`: low-level HTTP endpoint and JSON request/response utilities.
- `src/backend/diagnostics.rs`: external backend diagnostics.

Expected benefit:
- Lower cognitive load for inference changes.
- Better test targeting.
- Cleaner future support for additional remote backends.

### 2. Split `src/model_catalog.rs` DONE

Current state:
- `src/model_catalog.rs` owns model discovery, GGUF metadata parsing, tokenizer inference, metadata merge logic, routing decisions, family inference, workload inference, and JSON formatting for commands.

Why refactor:
- This is a classic accumulation file with multiple reasons to change.
- Model discovery and model routing evolve at different speeds.
- Metadata parsing is now important enough to deserve isolated tests and its own abstractions.

Recommended split:
- `src/model/catalog.rs`: discovery and refresh.
- `src/model/metadata.rs`: GGUF and tokenizer metadata extraction, merge logic, normalization.
- `src/model/routing.rs`: driver resolution, workload-aware routing, target selection.
- `src/model/workload.rs`: workload parsing and inference helpers.
- `src/model/formatting.rs`: JSON and status formatting for command responses.

Expected benefit:
- Smaller APIs per domain.
- Easier addition of new metadata sources.
- Reduced duplication between workload parsing call sites.

### 3. Reduce `src/main.rs` to bootstrap only DONE

Current state:
- `src/main.rs` contains kernel bootstrap, TCP server setup, event loop, auth-token provisioning, checkpoint scheduling, model catalog initialization, worker setup, and shutdown handling.

Why refactor:
- The file currently acts as both binary entry point and kernel runtime implementation.
- Startup policy, event-loop orchestration, and operational concerns are tightly coupled.
- Configuration centralization improved this, but the file is still doing too much.

Recommended split:
- `src/kernel/bootstrap.rs`: startup, config load, auth token preparation, listener creation.
- `src/kernel/server.rs`: event loop and client lifecycle.
- `src/kernel/checkpointing.rs`: periodic checkpoint logic and snapshot assembly.
- `src/main.rs`: tracing init plus call into bootstrap.

Expected benefit:
- Cleaner startup code.
- Easier unit testing of bootstrap logic.
- Better isolation of operational behavior from the binary entry point.

## Runtime And Execution Flow

### 4. Extract syscall handling from `src/runtime.rs` DONE

Current state:
- `src/runtime.rs` combines inference result handling, process teardown, orchestration progression, token quota enforcement, syscall interception, and output delivery.

Why refactor:
- Runtime tick logic is central and performance-sensitive.
- Mixed responsibilities make it easy to regress unrelated paths when changing one execution stage.
- Syscall interception is logically independent from inference result delivery.

Recommended split:
- `src/runtime/tick.rs`: top-level phase ordering.
- `src/runtime/inference_results.rs`: worker result application.
- `src/runtime/syscalls.rs`: syscall buffer scanning and dispatch integration.
- `src/runtime/orchestration.rs`: orchestration progress handling.

Expected benefit:
- More testable runtime phases.
- Lower regression surface.
- Easier instrumentation of each phase.

### 5. Simplify `src/tools.rs` DONE

Current state:
- `src/tools.rs` contains workspace path policy, sandbox selection, rate limiting, timeout execution, audit logging, stale-script cleanup, and Python tool execution.

Why refactor:
- It mixes policy, filesystem security, subprocess execution, and telemetry.
- Rate-limit state and sandbox policy are orthogonal concerns.
- This file is a likely hotspot for accidental security regressions.

Recommended split:
- `src/tools/policy.rs`: rate limit and sandbox config.
- `src/tools/path_guard.rs`: safe path resolution and workspace root handling.
- `src/tools/python_runner.rs`: script generation and execution.
- `src/tools/audit.rs`: audit log persistence.

Expected benefit:
- Safer review boundaries.
- Clearer ownership of sandbox logic.
- Less accidental coupling between tool behavior and filesystem rules.

### 6. Deduplicate checkpoint snapshot assembly DONE

Current state:
- Snapshot building logic is duplicated between `src/main.rs` and `src/commands/checkpoint_cmd.rs`.

Why refactor:
- The same kernel state is serialized through two separate assembly flows.
- Any future snapshot field risks drifting between auto-checkpoint and manual checkpoint.

Recommended split:
- Add a single snapshot builder service, for example `src/checkpoint/builder.rs`, used by both manual and periodic checkpointing.

Expected benefit:
- One serialization path.
- Lower maintenance cost when snapshot schema evolves.

## Configuration And Operational Consistency

### 7. Finish config adoption outside the core kernel DONE

Current state:
- The kernel now reads `agenticos.toml` at startup.
- GUI and command-line clients partially read the shared config.
- Evaluation scripts in `src/eval_llama3.py` and `src/eval_swarm.py` still carry their own host, port, timeout, model-path, and report-path defaults.

Why refactor:
- Operational drift is still possible between runtime, GUI, and benchmark tooling.
- The repo still contains multiple entry points with partially duplicated transport defaults.

Recommended follow-up:
- Introduce one tiny shared Python config loader used by `gui/` and `src/*.py` utilities.
- Keep TOML as the source of truth and treat env vars as optional overrides only.

Expected benefit:
- Fewer environment-specific surprises.
- Easier reproducibility across GUI, CLI, and benchmarks.

## Efficiency And Hot Paths

### 8. Avoid repeated full-catalog recomputation on every refresh-heavy path DONE

Current state:
- `ModelCatalog::discover` and formatting helpers rebuild catalog state and response payloads from scratch.
- `EXEC` and GUI flows trigger frequent refreshes and status queries.

Why refactor:
- As the number of models grows, full rescans and repeated JSON assembly become more expensive.
- The current implementation is fine at small scale but does not age well with a large local model library.

Recommended refactor:
- Cache discovered entries keyed by directory mtime or file hash snapshot.
- Cache formatted LIST/INFO payloads and invalidate only on catalog refresh.

Expected benefit:
- Better responsiveness in model-heavy workspaces.
- Less repeated filesystem churn.

### 9. Review prompt/token conversions around remote inference DONE

Current state:
- The external backend reconstructs prompts from tokens and may tokenize emitted text when token ids are absent.
- This is necessary for compatibility, but it is also a frequent source of subtle bugs.

Why refactor:
- Prompt reconstruction, completion parsing, and stop handling are part of a fragile adapter layer.
- This code deserves a tighter abstraction boundary and dedicated fixtures.

Recommended refactor:
- Formalize a transport adapter contract for remote backends.
- Move prompt serialization and completion decoding into explicit adapter utilities.
- Add fixture-based tests using captured `llama-server` payloads.

Expected benefit:
- Easier regression prevention for Qwen and future reasoning models.

## GUI Architecture

### 10. Reduce `gui/app.py` DONE

Current state:
- `gui/app.py` coordinates layout, command dispatch, retry behavior, worker threads, state transitions, and widget updates.

Why refactor:
- It acts as controller, service layer, and view orchestrator simultaneously.
- Retry logic and transport error handling are repeated in multiple methods.

Recommended split:
- `gui/request_handler.py`: retries, timeouts, async request dispatch.
- `gui/session_state.py`: selected model, active process state, connection state.
- `gui/app.py`: UI wiring only.

Expected benefit:
- Simpler window logic.
- Lower risk when modifying connection behavior.

### 11. Centralize GUI response parsing DONE

Current state:
- `gui/widgets/models.py`, `gui/widgets/processes.py`, `gui/widgets/memory.py`, and `gui/widgets/chat.py` parse different slices of kernel responses independently.

Why refactor:
- Schema knowledge is spread across widgets.
- Any STATUS or LIST response evolution forces multiple manual updates.

Recommended split:
- `gui/response_parser.py`: normalized parser functions for STATUS, LIST_MODELS, CHECKPOINT, and orchestration outputs.

Expected benefit:
- Single source of truth for kernel response decoding.
- Less widget-specific string parsing.

### 12. Reorganize the `gui/widgets/` folder by responsibility DONE

Current state:
- `gui/widgets/` contains domain panels, layout helpers, and supporting widgets in one flat folder.

Why refactor:
- The folder no longer communicates ownership clearly.
- Flat growth makes reuse and onboarding harder.

Recommended layout:
- `gui/sections/`: chat, models, processes, memory, orchestration, logs.
- `gui/widgets/`: reusable visual controls only.
- `gui/services/`: protocol client wrappers, request handling, parser layer.

Expected benefit:
- Better navigability.
- Cleaner UI/service separation.

## Command Layer And Domain Boundaries

### 13. Reduce parsing duplication across commands and orchestration DONE

Current state:
- Workload parsing and generation defaults are consumed from several places: command handlers, orchestration, model catalog, scheduler, and prompting.

Why refactor:
- Behavior is currently correct but spread across modules that should not all own policy decisions.
- Policy drift is likely as more routing logic is added.

Recommended split:
- Create a small `src/policy/` module for workload normalization, generation defaults, and scheduler quota defaults.

Expected benefit:
- One place for operational policy.
- Cleaner distinction between domain policy and execution code.

### 14. Revisit command module folder structure DONE

Current state:
- `src/commands/` is already split, but several handlers still reach deeply into engine, memory, scheduler, orchestrator, and checkpoint internals.

Why refactor:
- Command handlers should coordinate services, not rebuild domain state themselves.
- The current `CommandContext` is useful, but some handlers still know too much about downstream storage structures.

Recommended direction:
- Keep transport parsing in `commands/`.
- Move reusable domain operations into dedicated services consumed by command handlers.

Expected benefit:
- Thinner command handlers.
- Less churn when domain internals change.

## Suggested Refactor Order

### Phase A: Low-risk structural wins

1. Extract checkpoint snapshot builder.
2. Split `src/tools.rs` by concern.
3. Centralize GUI response parsing.
4. Move config loading for Python evaluation scripts to the shared TOML source.

### Phase B: High-value core cleanup

1. Split `src/backend.rs`.
2. Split `src/model_catalog.rs`.
3. Reduce `src/main.rs` to bootstrap only.
4. Extract runtime phases from `src/runtime.rs`.

### Phase C: UI architecture cleanup

1. Shrink `gui/app.py`.
2. Reorganize `gui/widgets/` into sections, widgets, and services.

## Notes

- The recent `agenticos.toml` work improves runtime consistency, but it also highlights where configuration ownership is still spread across the repository.
- The most urgent structural risk remains the adapter boundary around remote inference in `src/backend.rs` and the size of `src/model_catalog.rs`.
- No refactors from this plan are implemented in this document; this is an analysis and sequencing artifact only.