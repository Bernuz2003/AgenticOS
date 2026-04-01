# Local Backend Quality Analysis

## Symptom

The persisted assistant message for `sess-2-000001` in `workspace/agenticos.db` was visibly degraded:

- internal reasoning leaked into the user-visible answer
- repeated tool prefixes were collapsed (`TOOLask_human`, `calcexpression`, etc.)
- punctuation and spacing were corrupted
- the turn was much slower than expected

## Evidence From The Main DB

`session_messages` for `sess-2-000001` contained:

- a visible `<think> ... </think>` block
- malformed tool listings such as `TOOLask_human` instead of `TOOL:ask_human`
- collapsed punctuation around tool descriptions

`audit_events` for the same session showed:

- repeated `first_chunk_received`
- final `output_turn_completed`
- elapsed time around `1051s`

This confirmed the session was real and not a GUI-only rendering problem.

## Root Cause

The degraded output came from three kernel/backend issues interacting together.

### 1. The local backend was reinjecting reasoning into visible assistant text

`crates/agentic-kernel/src/backend/local/remote_adapter.rs`

We were taking `reasoning_content` returned by the local `llama.cpp` server and wrapping it into:

- `<think> ... </think>`

then prepending it to the assistant-visible text.

That is why the DB captured English internal reasoning for a simple Italian question.

### 2. The local transport normalizer was dropping legitimate repeated fragments

`crates/agentic-kernel/src/backend/local/llamacpp.rs`

`canonical_transport_delta(...)` treated any fragment already present anywhere in the accumulated text as duplicate:

- `current.contains(fragment)`
- `current.starts_with(fragment)`
- `current.ends_with(fragment)`

This is unsafe for normal prose and especially unsafe for tool inventories, where fragments like:

- `TOOL:`
- backticks
- punctuation
- JSON punctuation

repeat naturally.

That logic explains malformed strings such as:

- `TOOLask_human`
- `calcexpression`
- missing separators and punctuation

The corruption was therefore not just "Qwen writing badly"; we were mutating the streamed text incorrectly.

### 3. Qwen3 was still running in a thinking-oriented prompt regime

The local Qwen3.5 model had no explicit sidecar chat template in our catalog metadata, so the kernel fell back to a simpler generic Qwen prompt path.

That meant we were not emitting the "disable thinking" assistant preamble for Qwen3.

Combined with point 1, this made the local backend expose reasoning directly in user-visible output.

## Fix Implemented

### A. Reasoning is no longer rendered as assistant text

Updated:

- `crates/agentic-kernel/src/backend/local/remote_adapter.rs`

`combine_completion_text(...)` now uses assistant content only and ignores `reasoning_content` for user-visible output.

### B. Transport normalization no longer drops repeated legitimate fragments

Updated:

- `crates/agentic-kernel/src/backend/local/llamacpp.rs`

Removed the unsafe duplicate checks based on:

- `current.contains(fragment)`
- `current.starts_with(fragment)`
- `current.ends_with(fragment)`

This keeps repeated `TOOL:`-like fragments intact.

### C. Qwen3 local metadata now uses a chat template that disables visible thinking

Updated:

- `models/qwen3.5-9b/metadata.json`
- `crates/agentic-kernel/src/prompt/rendering.rs`

The renderer now passes `enable_thinking = false` to Jinja templates, and the Qwen3.5 sidecar metadata now supplies a chat template that emits the assistant preamble expected to suppress visible thinking.

## Automated Coverage Added

Regression coverage now includes:

- `crates/agentic-kernel/tests/e2e/local_transport.rs`
  - repeated `TOOL:` fragments are preserved
- `crates/agentic-kernel/tests/e2e/prompt_rendering.rs`
  - Qwen Jinja generation prompt disables thinking
- updated backend tests for ignored `reasoning_content`

`cargo test -p agentic-kernel` passes after the fix.

## Live Verification After The Fix

A fresh isolated kernel probe was started with:

- `AGENTIC_PORT=6391`
- `AGENTIC_DB_PATH=/tmp/agenticos-quality-probe-2.db`
- `AGENTIC_LLAMACPP_PORT_BASE=8091`

### Real protocol probe: normal question

Prompt:

- `Quali sono i tool che hai a disposizione?`

Observed TCP `DATA` frames now begin with:

- `Ho`
- ` a`
- ` disposizione`
- ` i`
- ` seguenti`
- ` tool`
- `:`

and then continue with correctly separated entries like:

- `` `TOOL:list_files` ``
- `` `TOOL:read_file` ``

Key result:

- no `<think>` in the visible stream
- no `TOOLask_human`
- no `TOTOOLOL`

### Real protocol probe: rigid tool marker

Prompt:

- `Rispondi esattamente con TOOL:calc {"expression":"1847*23"} e nulla altro.`

Persisted result in `/tmp/agenticos-quality-probe-2.db`:

- invocation event command `TOOL:calc {"expression":"1847*23"}`
- audit `dispatched`
- audit `completed`

Key result:

- canonical tool command dispatched end-to-end
- no corrupted marker in persisted invocation state

### Fresh DB checks

In `/tmp/agenticos-quality-probe-2.db`:

- `%TOTOOLOL%` => `0`
- `%<think>%` => `0`
- `%TOOLask_human%` => `0`

## What Explains The Remaining Slowness

The remaining performance problem is real and separate from the text corruption.

`workspace/local-runtimes/logs/qwen.log` still shows that Qwen3.5 on the current `llama.cpp` local runtime path is expensive because the server keeps re-entering `/completion` and re-evaluating large prompts.

Observed timings from the fresh probe:

- `prompt eval time = 104555.33 ms / 504 tokens`
- `eval time = 26212.24 ms / 64 tokens`

and for the rigid marker prompt:

- `prompt eval time = 107499.94 ms / 516 tokens`
- `eval time = 5532.46 ms / 16 tokens`

So:

- text corruption was caused by our backend/kernel path and is fixed here
- visible reasoning leakage was caused by our backend/kernel path and is fixed here
- slow generation is still fundamentally limited by the current local Qwen3.5 + `llama.cpp` runtime behavior

## Conclusion

The bad answer in `sess-2-000001` was not just the model "suddenly getting worse".

It was caused by:

1. visible reasoning leakage
2. unsafe local stream deduplication
3. missing Qwen3 no-thinking prompt path

Those regressions are now fixed and validated on:

- the main DB evidence
- automated tests
- direct TCP black-box probing
- isolated DB/audit output
