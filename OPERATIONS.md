# AgenticOS Operations

This runbook covers the two production-facing areas introduced with the SQLite control plane:

- backup and restore of `workspace/agenticos.db`;
- local capacity policy for RAM/VRAM admission control.

## Authoritative state

The kernel now treats `workspace/agenticos.db` as the authoritative local store for:

- sessions and process runs;
- timeline history and session replay;
- runtime inventory and runtime load queue;
- append-only accounting;
- append-only audit events;
- boot and recovery metadata.

Legacy files such as `timeline_sessions/*.json` and `workspace/syscall_audit.log` may still exist for compatibility or mirroring, but they are not the primary state source anymore.

## SQLite backup

Recommended path for a cold backup:

1. Stop the kernel and the Tauri workspace.
2. Copy `workspace/agenticos.db` to a backup location.
3. Keep backups versioned by timestamp.

Example:

```bash
mkdir -p backups
cp workspace/agenticos.db "backups/agenticos-$(date +%Y%m%d-%H%M%S).db"
```

If a live backup is required and `sqlite3` is available, use SQLite's backup path instead of copying open WAL files:

```bash
mkdir -p backups
sqlite3 workspace/agenticos.db ".backup 'backups/agenticos-live-$(date +%Y%m%d-%H%M%S).db'"
```

Operational notes:

- prefer cold backups before schema upgrades or milestone rollouts;
- do not treat `agenticos.db-wal` or `agenticos.db-shm` as standalone backups;
- if the kernel is stopped cleanly, copying the main `.db` file is sufficient.

## SQLite restore

Recommended restore flow:

1. Stop the kernel and the Tauri workspace.
2. Move the current DB aside as rollback material.
3. Copy the chosen backup into `workspace/agenticos.db`.
4. Restart the kernel.
5. Verify lobby sessions, runtime queue, accounting totals, and recent audit events.

Example:

```bash
mv workspace/agenticos.db "workspace/agenticos.db.rollback-$(date +%Y%m%d-%H%M%S)"
cp backups/agenticos-20260313-120000.db workspace/agenticos.db
```

After restore, the next kernel boot will run normal SQLite migrations and boot recovery. Interrupted runs are not resumed as live PIDs; sessions are recovered logically from persisted history.

## Capacity policy for local runtimes

The `ResourceGovernor` is the admission control layer for local models. It prevents overcommit by reserving RAM/VRAM before a runtime can move from `queued` or `loading` to `ready`.

Primary knobs in [config/kernel/base.toml](/home/bernuz/Progetti/AgenticOS/agenticOS/config/kernel/base.toml):

- `resources.ram_budget_bytes`
- `resources.vram_budget_bytes`
- `resources.min_ram_headroom_bytes`
- `resources.min_vram_headroom_bytes`
- `resources.local_runtime_ram_scale`
- `resources.local_runtime_vram_scale`
- `resources.local_runtime_ram_overhead_bytes`
- `resources.local_runtime_vram_overhead_bytes`
- `resources.max_queue_entries`

Recommended policy:

- set explicit RAM and VRAM budgets on machines expected to load more than one local model;
- keep non-zero headroom to protect the OS and the GUI from starvation;
- size budgets below physical capacity so swap, compositor, and background processes still fit;
- pin only runtimes that must never be evicted, because pinned runtimes can force queueing or refusal for other loads;
- monitor runtime queue growth as an early signal that budgets are too tight or models are too large.

## Tuning guidance

Use these defaults as starting points:

- `local_runtime_ram_scale = 1.15`
- `local_runtime_vram_scale = 1.05`
- `local_runtime_ram_overhead_bytes = 268435456`
- `local_runtime_vram_overhead_bytes = 134217728`

When to tune:

- increase scales or overhead if runtime loads succeed but generation becomes unstable under concurrent pressure;
- decrease scales only after measuring real resident usage on the target hardware;
- increase `min_*_headroom_bytes` on desktop machines where the GUI, compositor, or other GPU workloads must remain responsive;
- keep `max_queue_entries` bounded so the kernel refuses impossible work quickly instead of accumulating an unbounded backlog.

## Runtime admission outcomes

For local runtime requests, one of these outcomes should occur:

- `ready`: reservation granted and runtime loaded;
- `eviction_started`: idle runtime eviction made room for the new load;
- `queued`: request is persisted in `runtime_load_queue` until capacity becomes available;
- `refused`: request exceeds budget or the queue is full.

These outcomes are visible in kernel status, audit, and the workspace lobby.

## Recovery expectations

After reboot or restore:

- sessions, accounting, audit, and queue state are reloaded from SQLite;
- remote stateless backends resume logically from persisted context only;
- local resident backends may be marked as strong-restore candidates, but live PID resurrection is not promised unless the backend explicitly supports it.
