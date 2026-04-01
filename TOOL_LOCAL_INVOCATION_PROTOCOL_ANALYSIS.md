# Tool Invocation Local Protocol Analysis

## Scope

This note reopens the local `TOOL:` corruption bug using the real kernel TCP protocol, not only internal harnesses.

## Evidence Collected

### Direct TCP protocol

The real `EXEC` path still reproduced a corrupted marker on local Qwen:

- prompt: `Rispondi esattamente con TOOL:calc {"expression":"1847*23"} e nulla altro.`
- observed TCP `DATA` frames:
  - `TO`
  - `TOOLOL`
  - `:`
  - `calc`
  - ...
- persisted assistant message for `sess-1-000005`:
  - `<think>\n\n</think>\n\nTOTOOLOL:calc {"expression":"1847*23"}`

This proves the bug is upstream of bridge/GUI reconstruction.

### Why previous live tests were misleading

The previous `run_live_local_completion(...)` harness did **not** exercise the same transport regime as the kernel:

- real kernel local runtime used `external_llamacpp.chunk_tokens = 1`
- the live helper used `ExternalLlamaCppBackend::for_diagnostics(..., 64)`

So the passing live helper was measuring a different system.

### Runtime log evidence

`workspace/local-runtimes/logs/qwen.log` shows the real runtime issuing many sequential `/completion` requests with nearly identical growing prompts:

- prompt token counts like `519 -> 522 -> 526 -> 527 -> 528 -> 533 -> 534`
- each request is a fresh completion call
- generation defaults still include temperature/top-p sampling

This is consistent with one-token-at-a-time stepping and repeated re-sampling across HTTP requests.

## Root Cause

The issue is not bridge or parser logic.

The problem was that the real local runtime path was still configured for **one-token HTTP stepping**:

- `chunk_tokens = 1`
- each step re-enters `/completion`
- sampling is restarted for each step
- the resulting text is not equivalent to one coherent streamed generation

That made control markers especially fragile on local Qwen, and explains why:

- direct backend diagnostics with a larger chunk budget looked canonical
- the real kernel TCP path still produced `TOTOOLOL:`

## Fix Direction

1. Make the local runtime use the same coherent streaming regime in production and in live tests.
2. Remove the hidden divergence between diagnostics and runtime.
3. Verify the result again on:
   - direct TCP protocol
   - kernel e2e tests
   - persisted DB output

## Implemented in This Milestone

- aligned `run_live_local_completion(...)` with the configured runtime `chunk_tokens`
- raised local `external_llamacpp.chunk_tokens` default from `1` to `64`
- added ignored live TCP black-box tests in `crates/agentic-kernel/tests/e2e/live_protocol.rs`
- moved TCP debug tooling into `tools/debug/`

## Final Verification

A fresh kernel instance was started on:

- kernel TCP port `6391`
- local llama.cpp port base `8091`
- isolated DB `/tmp/agenticos-protocol-probe.db`

Observed results:

- direct TCP protocol no longer emitted `TOTOOLOL:`
- raw `DATA` frames contained canonical `TOOL:` text inside reasoning output
- DB persistence for the fresh probe contained canonical invocation events
- `select count(*) from session_messages where content like '%TOTOOLOL%'` returned `0`
- the new ignored black-box tests `live_protocol_rigid_prompt_preserves_canonical_tool_marker` and `live_protocol_natural_prompt_preserves_canonical_tool_marker` both passed against that fresh kernel

Related cleanup also landed:

- removed duplicate tracing initialization from `crates/agentic-kernel/src/main.rs`
- moved protocol debug tooling under `tools/debug/`
