# AgenticOS — Roadmap Operativa

Questo file è la fonte unica di verità per il piano del progetto.

## Come usarla

- Prima di iniziare ogni slice, rileggere questa roadmap insieme a `CRITICITY_TO_FIX.md`.
- Aggiornare lo stato di ogni punto a fine attività (`TODO` → `IN_PROGRESS` → `DONE`).
- Aggiornare a fine slice sia la roadmap sia `CRITICITY_TO_FIX.md` con delta, DoD e validazione eseguita.
- Registrare data, note sintetiche e commit/riferimenti utili.
- Non aprire un nuovo punto senza una **Definition of Done (DoD)** verificabile.

---

## Stato attuale (snapshot)

- **Data snapshot:** 2026-03-07
- **Versione:** `v0.5.0`
- **Runtime:** server TCP event-driven (`mio 1.0`) + engine LLM process-centric (`candle 0.9`)
- **Codebase:** ~5.800 righe Rust (kernel) + ~1.300 righe Python (GUI PySide6)
- **Test suite:** 109 test verdi (`cargo test --release`), clippy con soli warning di debito minore residuo, CI GitHub Actions
- **Modelli supportati:** Llama 3.1 (Q4_K_M) + Qwen 2.5 (Q4_K_M) con auto-discovery e capability routing
- **Dipendenze chiave:** `candle-core/candle-transformers 0.9.1`, `tokenizers 0.22.2`, `mio 1.0`, `thiserror 2.0`, `tracing 0.1`, `serde 1.0`, `serde_json 1.0`

---

## Archivio milestone completate (1–12)

Le milestone 1–12 coprono la costruzione dell'intero kernel single-node, dalla stabilità base alla GUI desktop. Sono tutte **DONE** e qui riassunte per riferimento.

| # | Titolo | Completata | Sintesi | File principali |
|---|--------|------------|---------|-----------------|
| 1 | Stabilità core runtime | 2026-02-22 | Flush I/O affidabile, cleanup zombie PID, marker `PROCESS_FINISHED`, gestione `ConnectionReset`/`BrokenPipe` | `main.rs`, `runtime.rs` |
| 2 | Hardening protocollo TCP | 2026-02-22 | Header strict, reply codificate con lunghezza, framing `DATA raw <len>`, errori `-ERR CODE` | `protocol.rs`, `transport/framing.rs` |
| 3 | Qualità inferenza Llama 3 | 2026-02-22 | Parametri sampling runtime (`GET_GEN`/`SET_GEN`), stop policy per-family, benchmark baseline replicabile | `backend.rs`, `prompting.rs` |
| 4 | Hardening SysCall | 2026-02-23 | Timeout enforced, path safety, audit log, sandbox mode (`host`/`container`/`wasm`), rate-limit + kill su abuso | `tools.rs` |
| 5 | Test & regressione | 2026-02-22 | Unit/integration/e2e, multi-client, reconnect, benchmark commit-aware (`reports/`) | `transport/mod.rs` (test) |
| 6 | Osservabilità operativa | 2026-02-23 | `STATUS`/`TERM`/`KILL`/`SHUTDOWN`, metriche aggregate, logging strutturato, graceful shutdown | `commands/mod.rs`, `commands/metrics.rs` |
| 7 | Refactoring multi-LLM | 2026-02-23 | Catalogo auto-discovery, `LIST_MODELS`/`SELECT_MODEL`/`MODEL_INFO`, prompt-family abstraction, backend Qwen, capability scheduler v1 | `model_catalog.rs`, `prompting.rs`, `backend.rs` |
| 8 | NeuralMemory nel runtime | 2026-02-23 | Ownership PID, quote token-slot, OOM events, MEMW, swap asincrono worker queue, eviction LRU, path safety I/O, fallback no-op | `memory/core.rs`, `memory/eviction.rs`, `memory/swap_io.rs`, `memory/types.rs` |
| 9 | Scheduler & Swarm | 2026-03-04 | `ProcessScheduler` con priorità (Low→Critical), quote token/syscall per WorkloadClass, enforcement runtime, opcodes `SET_PRIORITY`/`GET_QUOTA`/`SET_QUOTA`, STATUS arricchito | `scheduler.rs`, `runtime.rs`, `commands/mod.rs` |
| 10 | GUI desktop | 2026-02-23 | PySide6: start/stop kernel, EXEC streaming, TERM/KILL, STATUS polling, pannello modelli/generation, export diagnostico, runbook E2E | `gui/app.py`, `gui/protocol_client.py`, `gui/kernel_manager.py` |
| 11 | Refactoring qualità | 2026-03-04 | `config.rs` centralizzato, versione da `CARGO_PKG_VERSION`, estrazione funzioni da `runtime.rs`, `memory/types.rs`, +17 test (54 totali) | `config.rs`, `runtime.rs`, `memory/types.rs` |
| 12 | Hardening architetturale | 2026-03-04 | `thiserror` error hierarchy (`MemoryError`, `CatalogError`, `ProtocolError`, `EngineError`), `tracing` structured logging, `struct Kernel`, `memory/eviction.rs` estratto, CI GitHub Actions | `errors.rs`, `main.rs`, `.github/workflows/ci.yml` |

**Totale test accumulati:** 37 (fine M8) → 54 (fine M11/M12) → 67 (fine M9-scheduler) → 74 (fine M14-checkpoint) → 94 (fine M16-orchestration)

---

## Mappa codebase corrente

```
src/
├── main.rs              # struct Kernel, event loop mio, auto-checkpoint timer (~280 righe)
├── checkpoint.rs        # KernelSnapshot, save/load atomic, snapshot builders (~340 righe)
├── orchestrator.rs      # Orchestrator, TaskGraphDef, DAG validation, advance logic (~500 righe)
├── protocol.rs          # OpCode enum (incl. Checkpoint/Restore/Orchestrate), CommandHeader parser, response formatting
├── config.rs            # env_bool, env_u64, env_usize centralizzati
├── errors.rs            # KernelError, MemoryError, EngineError, ProtocolError, CatalogError
├── model_catalog.rs     # ModelCatalog auto-discovery, WorkloadClass, capability routing
├── prompting.rs         # PromptFamily (Llama/Qwen/Mistral), system/user templates, stop policy
├── backend.rs           # RuntimeModel trait, dispatch per famiglia (quantized_llama/qwen2)
├── process.rs           # AgentProcess, ProcessState, SamplingParams
├── runtime.rs           # run_engine_tick, dispatch_process_syscall, orchestration advance (~350 righe)
├── scheduler.rs         # ProcessScheduler, ProcessPriority, ProcessQuota, ResourceAccounting
├── tools.rs             # Syscall sandbox (python/write_file/calc), rate-limit, audit
├── commands/
│   ├── mod.rs           # execute_command dispatch incl. CHECKPOINT/RESTORE/ORCHESTRATE
│   ├── context.rs       # CommandContext<'a>: &mut NeuralMemory, &mut Option<LLMEngine>, &mut MetricsState
│   ├── metrics.rs       # MetricsState (owned by Kernel), record/snapshot methods
│   └── parsing.rs       # parse_generation_payload, parse_memw_payload
├── engine/
│   ├── mod.rs           # LLMEngine, spawn/step/kill process
│   ├── lifecycle.rs     # load_engine_from_catalog
│   └── tokenizer.rs     # load_tokenizer, validate_chat_template
├── memory/
│   ├── mod.rs           # re-export
│   ├── types.rs         # TensorId, MemorySnapshot, SwapEvent, MemoryConfig
│   ├── core.rs          # NeuralMemory allocatore (~680 righe)
│   ├── swap.rs          # SwapManager (worker, queue, polling, path validation)
│   ├── eviction.rs      # LRU eviction (clear/touch/victim/evict_until_fit)
│   └── swap_io.rs       # Swap I/O worker, path validation, atomic write
└── transport/
    ├── mod.rs           # re-export + test integration TCP (~530 righe)
    ├── client.rs        # Client struct (stream, buffers)
    ├── framing.rs       # parse_available_commands, frame parsing
    └── io.rs            # handle_read, raw TCP dispatch

gui/
├── app.py               # PySide6 main application
├── protocol_client.py   # TCP client con framing protocol
├── kernel_manager.py    # Kernel lifecycle manager (start/stop)
└── README.md            # Runbook operativo
```

---

## Roadmap attiva — Fase 2: Consolidamento + Orchestrazione

### 13) Swap extraction — `memory/swap.rs`
**Status:** `DONE` ✅

**Obiettivi**
- Estrarre la logica swap worker da `memory/core.rs` in modulo dedicato `memory/swap.rs`.
- Ridurre `core.rs` sotto le 450 righe, migliorando manutenibilità.
- Nessun cambio funzionale: puro refactoring strutturale.

**DoD**
- [x] Modulo `memory/swap.rs` creato con: `SwapManager` (configure, enqueue, poll_events, is_pid_waiting, remove_waiting, persist_payload).
- [x] `memory/core.rs` produzione sotto 450 righe (424), con `SwapManager` come campo `swap`.
- [x] `memory/mod.rs` aggiornato con `pub(crate) mod swap`.
- [x] Suite test invariata e verde (67/67).
- [x] Clippy pulito.

**Esito**
- `core.rs`: 809 → 673 righe (424 produzione + 249 test). Rimossi: `SwapJob`, `SwapResult`, `configure_async_swap` body, `poll_swap_events` body, `enqueue_swap`, campi raw swap.
- `swap.rs`: 236 righe — `SwapManager` struct con stato swap encapsulato, worker thread spawn, job queue, completion polling con `SwapCounterDeltas`.
- Cambio architetturale: `NeuralMemory.swap: SwapManager` sostituisce 5 campi sparsi (`swap_enabled`, `swap_dir`, `swap_tx`, `swap_rx`, `waiting_for_memory`).

---

### 14) Persistence & Snapshot — checkpoint/restore stato kernel
**Status:** `DONE` ✅

**Obiettivi**
- Permettere salvataggio periodico dello stato kernel su disco (checkpoint).
- Permettere restore dello stato al boot (processi attivi, scheduler, metriche).
- Garantire che un crash non perda l'intero contesto di lavoro.

**DoD**
- [x] **14.1** Definire `KernelSnapshot` serializzabile (`serde`) con: processi attivi (PID, stato, prompt, workload), scheduler state (priorità, quote, accounting), metriche correnti.
- [x] **14.2** Comando protocollo `CHECKPOINT` che scrive snapshot su `workspace/checkpoint.json` (atomic write via temp+rename).
- [x] **14.3** Checkpoint automatico periodico configurabile via `AGENTIC_CHECKPOINT_INTERVAL_SECS` (default: disabilitato, 0).
- [x] **14.4** `Kernel::run_auto_checkpoint()` che nel loop salva stato dal vivo. Processi non ripristinabili marcati come `Orphaned` con log warning al restore.
- [x] **14.5** Comando protocollo `RESTORE` che ricarica scheduler entries (priorità, quote) da checkpoint. Model weights non inclusi — richiede `LOAD` manuale.
- [x] **14.6** Test unitari: serializzazione/deserializzazione roundtrip, checkpoint atomico, restore con dati corrotti (graceful fallback), timestamp epoch, protocol opcode parsing, registered_pids.
- [x] **14.7** Suite test verde (74/74), clippy pulito.

**Dipendenze nuove:** `serde 1.0`, `serde_json 1.0`

**Esito**
- `src/checkpoint.rs` (~340 righe): `KernelSnapshot` top-level + 6 sub-snapshot types (`ProcessSnapshot`, `SchedulerStateSnapshot`, `SchedulerEntrySnapshot`, `GenerationSnapshot`, `MetricsSnapshot`, `MemoryCountersSnapshot`). Atomic write via temp+rename. Builder functions `snapshot_scheduler()` e `snapshot_memory()`. 5 unit test.
- `src/protocol.rs`: +2 opcodes `Checkpoint`/`Restore`, match arms in parser.
- `src/commands/mod.rs` (~650 righe): +CHECKPOINT handler (builds snapshot from live state, atomic save), +RESTORE handler (reloads scheduler entries with priorities/quotas, restores selected model in catalog).
- `src/main.rs` (~270 righe): `checkpoint_interval_secs` + `last_checkpoint` fields in `Kernel`, `run_auto_checkpoint()` method in event loop (best-effort, errors logged).
- `src/scheduler.rs`: `registered_pids()` method for checkpoint serialization.
- Test: 67 → 74 (+5 checkpoint, +1 protocol opcode, +1 scheduler registered_pids).
- Design decision: checkpoint captures metadata only (scheduler, process list, config, metrics). Model weights and tensor data NOT included — processes marked `Orphaned` on restore. This avoids massive serialization overhead.

---

### 15) Documentazione architetturale — ARCHITECTURE.md
**Status:** `DONE` ✅

**Obiettivi**
- Creare un documento di design che descriva l'architettura complessiva del sistema.
- Rendere il progetto comprensibile a contributor esterni o a sé stessi tra 6 mesi.
- Includere diagrammi a blocchi e flussi end-to-end.

**DoD**
- [x] **15.1** `ARCHITECTURE.md` nella root con: overview sistema, diagramma a blocchi (kernel, engine, memory, scheduler, transport, tools), glossario concetti chiave.
- [x] **15.2** Flusso end-to-end documentato: `LOAD` → `EXEC` → token generation → syscall dispatch → `PROCESS_FINISHED`.
- [x] **15.3** Sezione "Memory subsystem" con diagramma alloc/eviction/swap lifecycle.
- [x] **15.4** Sezione "Scheduler" con diagramma priority ordering + quota enforcement.
- [x] **15.5** Sezione "Protocol" con tabella completa opcodes, formato header/reply, esempi.
- [x] **15.6** Diagrammi in Mermaid (renderizzabili su GitHub).

**Esito**
- `ARCHITECTURE.md` (~500 righe): 14 sezioni — overview, diagramma blocchi Mermaid, event loop flowchart, sequence diagram LOAD→EXEC→FINISHED, tabella completa 18 opcodes con wire format ed esempi, architettura engine e processi con state machine, memory subsystem con flusso write/eviction/swap, scheduler con priority ordering e quota enforcement, syscall sandbox con security model, model catalog routing, checkpoint/restore, configurazione env vars completa, error hierarchy tree, glossario 20 termini.
- 8 diagrammi Mermaid: block diagram, event loop flowchart, end-to-end sequence, process state machine, memory write flow, scheduler enforcement flow, capability routing flow, error hierarchy tree.

---

### 16) Agent orchestration primitives — task graph
**Status:** `DONE` ✅

**Obiettivi**
- Introdurre primitive per orchestrazione multi-agente: un processo "orchestratore" lancia sub-task, raccoglie risultati, decide il passo successivo.
- Abilitare workflow strutturati (DAG di task) e non solo esecuzioni isolate.
- Mantenere l'approccio event-driven senza blocking nel loop principale.

**DoD**
- [x] **16.1** Tipo `TaskGraph` con nodi (task con prompt + workload hint + dipendenze) e archi (data flow).
- [x] **16.2** Opcode `ORCHESTRATE <json_task_graph>` che registra un grafo, spawna il primo livello di task indipendenti, e avanza automaticamente quando le dipendenze sono soddisfatte.
- [x] **16.3** Stato orchestrazione interrogabile via `STATUS <orchestration_id>` con progress (nodi completati/totali/falliti).
- [x] **16.4** Raccolta risultati: output di ogni sub-task accessibile dall'orchestratore come contesto per i nodi successivi.
- [x] **16.5** Policy fallimento configurabile: `fail_fast` (abort tutto al primo errore) vs `best_effort` (continua nodi indipendenti).
- [x] **16.6** Test unitari: grafo lineare (A→B→C), grafo parallelo (A→{B,C}→D), grafo con nodo fallito + policy.
- [x] **16.7** Suite test verde (94/94), clippy pulito.

**Note di design**
- L'orchestratore non è un processo LLM speciale: è logica kernel-side che gestisce lo scheduling dei sub-task usando il `ProcessScheduler` esistente.
- I risultati intermedi accumulati in buffer in-kernel leggero (`HashMap<String, String>`) nell'`Orchestration`. Output di dependency iniettato nel prompt dei nodi successori.
- Il formato task graph è JSON per interoperabilità con la GUI e client esterni.
- Validazione DAG: topological sort (Kahn's algorithm) con cycle detection, verifica dipendenze, self-dependency, duplicate task IDs.
- STATUS con prefisso `orch:N` per distinguere orchestrazioni da PID.

**Esito**
- `src/orchestrator.rs` (~500 righe): `TaskGraphDef`/`TaskNodeDef` (serde JSON-deserializable), `FailurePolicy` (`fail_fast`/`best_effort`), `Orchestrator` con `register`/`register_pid`/`append_output`/`mark_completed`/`mark_failed`/`advance`/`format_status`. DAG validation via topological sort. 20 unit test.
- `src/protocol.rs`: +1 opcode `Orchestrate`, +1 test.
- `src/commands/mod.rs` (~720 righe): +ORCHESTRATE handler (parse JSON, validate, spawn root tasks), +STATUS `orch:` prefix per orchestration query.
- `src/runtime.rs` (~350 righe): orchestrator output tracking in step loop, completion/failure notifications, advance+spawn after finished PIDs, fail_fast kill logic.
- `src/main.rs` (~280 righe): +`mod orchestrator`, `Orchestrator` field in Kernel, passed to `handle_read` e `run_engine_tick`.
- `src/transport/io.rs`: +`orchestrator: &mut Orchestrator` parameter.
- `src/transport/mod.rs` (~530 righe): 13 test calls updated, `setup_shared_state` returns Orchestrator.
- Test: 74 → 94 (+20 orchestrator tests: registration, linear/parallel advancement, fail_fast/best_effort policies, cycle/empty/duplicate/unknown-dep/self-dep validation, topo sort, status format, JSON deserialization, default policy, workload parsing, context building).

---

### 17) Benchmark comparativo swarm
**Status:** `DONE` ✅

**Obiettivi**
- Chiudere l'ultimo DoD rimasto dalla milestone 9 (benchmark swarm vs single-model).
- Produrre dati quantitativi su latenza, throughput e qualità con routing multi-modello vs modello singolo.
- Risolvere il crash OOM scoperto durante il primo tentativo di esecuzione.

**DoD**
- [x] Script benchmark riproducibile (`src/eval_swarm.py`).
- [x] Report JSON in `reports/swarm_benchmark.json` con metriche: first-token latency, throughput bytes/s, task completion rate.
- [x] Scenario A (single_model): 6 task su Llama 3.1 8B — eseguito con successo.
- [x] Scenario B (swarm_routing): codice completo ma skippato per limiti hardware (Qwen 14B richiede ~9 GB, swap da Llama non sufficiente su 31 GB con OS + VS Code + kernel in RAM).
- [x] Analisi regressione documentata nel report (null per assenza scenario B).
- [x] Fix kernel: **drop-before-load** in entrambi i handler `LOAD` e `EXEC` auto-switch.

**Fix critico: drop-before-load (OOM prevention)**

Il primo tentativo di esecuzione ha causato un crash di sistema (RAM saturata, CPU 100%, reboot necessario). Causa root: durante il model auto-switch nell'handler EXEC, la semantica Rust `*lock = Some(new_engine)` mantiene il vecchio engine in RAM fino a dopo l'assegnamento del nuovo. Con Llama (~5 GB) + Qwen (~9 GB) il picco raggiunge ~14 GB di soli pesi + overhead OS → OOM su 31 GB.

Fix applicato in `src/commands/mod.rs`:
```rust
// Drop-before-load: free old engine BEFORE allocating new one
{ let mut lock = engine_state.lock().unwrap(); *lock = None; }
// Now load — peak RAM = max(old, new), not old + new
match LLMEngine::load(...) { ... }
```

Applicato sia nel handler `LOAD` (riga ~57) che nel handler `EXEC` auto-switch (riga ~155). Sicuro nel design attuale (event loop single-threaded, nessun EXEC concorrente può osservare lo slot vuoto). Commentato con NOTE per futura migrazione async.

**Risultati Scenario A — Single model (Llama 3.1 8B Q4_K_M, CPU-only)**

| Task | Workload | First token (s) | Bytes ricevuti | Throughput (B/s) |
|------|----------|-----------------|----------------|------------------|
| brief_kernel | fast | 8.4 | 6697 | ~28 |
| code_fibonacci | code | 11.9 | 5328 | ~22 |
| reasoning_compare | reasoning | 19.6 | 4048 | ~17 |
| general_summary | general | 17.5 | 3702 | ~15 |
| code_python_sort | code | 14.0 | 3258 | ~14 |
| fast_translate | fast | 14.7 | 4727 | ~20 |

- **Sistema**: Ubuntu 24.04, i7-1355U (12 core, 15W TDP), 31 GB RAM, no GPU
- **Completion rate**: 0/6 — tutti i task hanno raggiunto il timeout di 240s perché senza `max_tokens` il modello genera indefinitamente (nessun `PROCESS_FINISHED` marker entro il timeout)
- **Tuttavia**: tutti i task producono output coerente e pertinente (verificato nei preview). Il modello funziona correttamente, il limite è la velocità CPU (~4-7 tok/s) senza cap sulla lunghezza.
- **Nessun crash**: il drop-before-load ha risolto il problema OOM. RAM stabile durante l'intera esecuzione (~24 min).

**Lezioni apprese**
1. **Max tokens obbligatorio per benchmark CPU**: senza limite, l'inference su i7 mobile non completa entro timeout ragionevoli. Per run futuri aggiungere `SET_GEN max_tokens=128`.
2. **Swarm routing richiede hardware upgrade**: il routing multi-modello con Qwen 14B (~9 GB) + overhead non è praticabile su 31 GB. Opzioni: (a) GPU dedicata (RTX 3060+), (b) 64 GB RAM, (c) modello più piccolo (Qwen 3B).
3. **Drop-before-load è sufficiente per single-thread**: nessun rischio di race condition nel design attuale. Diventerà critico con kernel async (documentato con TODO nel codice).

**Prerequisiti:** Almeno 2 modelli `.gguf` disponibili in `models/`.

---

## Roadmap attiva — Fase 2.5: GUI Redesign

### 10.1) GUI Redesign — Refactor strutturale
**Status:** `DONE` ✅

**Obiettivi**
- Riprogettare la GUI da zero: la versione M10 copre solo EXEC/Models/Logs, mancano 12 opcodes aggiunti da M9 in poi.
- Passare da layout a 3 tab generici a architettura sidebar + 6 sezioni dedicate.
- Look professionale con tema QSS dark, layout modulare con un file per sezione.

**DoD**
- [x] **10.1.1** Struttura file modulare: `gui/widgets/` con un file per sezione + `gui/styles/theme.qss`.
- [x] **10.1.2** Sidebar navigazione con 6 sezioni (Chat, Models, Processes, Memory, Orchestration, Logs) + mini-status sempre visibile (kernel state, model loaded, proc count, uptime).
- [x] **10.1.3** Sezione **Chat** funzionale: chat-style con bolle user/assistant, metriche inline (latency, tokens, throughput), dropdown workload hint (`auto/fast/code/reasoning/general`), barra generation params, PID management (TERM/KILL).
- [x] **10.1.4** Sezione **Models** funzionale: schede modello con stato (loaded/available/loading), routing map (workload→modello), pulsanti Load/Select/Info, warning RAM.
- [x] **10.1.5** Sezione **Logs** migrata: kernel log, syscall audit, filtri, export snapshot.
- [x] **10.1.6** Sezioni placeholder per Processes, Memory, Orchestration (UI scaffolding, "Coming in M10.2").
- [x] **10.1.7** Tema QSS dark professionale applicato globalmente.
- [x] **10.1.8** GUI si avvia senza errori, tutte le funzionalità migrate funzionano.

**Note di design**
- Stack: PySide6 (confermato), QSS per styling, QStackedWidget per routing sezioni.
- Architettura: MainWindow come controller, widgets comunicano via Qt signals, protocol calls in background threads con ui_queue.
- protocol_client.py e kernel_manager.py mantenuti con estensioni minime.

**Esito**
- `gui/styles/theme.qss` (~320 righe): Tokyo Night dark theme con stili per sidebar, buttons, inputs, cards, badges, scrollbars, tabs, tooltips.
- `gui/widgets/sidebar.py` (~160 righe): SidebarWidget con 6 nav buttons, mini-status panel (online/offline, model, procs, uptime), start/stop kernel.
- `gui/widgets/chat.py` (~260 righe): ChatSection con HTML bubbles, workload combo, gen params bar, PID TERM/KILL, metriche inline (elapsed, tok, tok/s).
- `gui/widgets/models.py` (~230 righe): ModelsSection con ModelCard (status badges), routing map, Load/Select/Info per card.
- `gui/widgets/logs.py` (~180 righe): LogsSection con kernel events + syscall audit, filtri stdout/stderr/noise, export snapshot.
- `gui/app.py` (~370 righe): MainWindow riscritto — sidebar + QStackedWidget, signal-based wiring, protocol dispatch centralizzato.
- Testato: imports OK, instantiation offscreen OK, navigation wiring OK, STATUS parsing OK, model card creation OK.

---

### 10.2) GUI Redesign — Sezioni avanzate
**Status:** `DONE` ✅

**Obiettivi**
- Completare le 3 sezioni nuove: Processes, Memory, Orchestration.
- Copertura completa di tutti i 20 opcodes del kernel.

**DoD**
- [x] **10.2.1** Sezione **Processes**: tabella processi attivi (PID, workload, priority, state, tokens, uptime), dettaglio processo selezionato con quota usage, azioni SET_PRIORITY/SET_QUOTA/TERM/KILL.
- [x] **10.2.2** Sezione **Memory**: barra visiva utilizzo NeuralMemory, stato swap, pulsanti CHECKPOINT/RESTORE, form MEMW manuale.
- [x] **10.2.3** Sezione **Orchestration**: visualizzazione stato DAG con progress per nodo, editor JSON per nuovi grafi, polling STATUS orch:N.
- [x] **10.2.4** Metriche inline nella Chat: latency, token count, throughput per ogni risposta.
- [x] **10.2.5** Protocol trace sub-tab in Logs (mostra req/resp raw).
- [x] **10.2.6** GUI si avvia senza errori, tutte le 20 opcode coperte.

**Esito**
- `gui/widgets/processes.py` (~260 righe): QTableWidget 7 colonne (PID/Workload/Priority/State/Tokens/Syscalls/Elapsed), detail panel con priority combo + quota inputs, azioni SET_PRIORITY/SET_QUOTA/GET_QUOTA/TERM/KILL/STATUS, scheduler summary.
- `gui/widgets/memory.py` (~220 righe): progress bar blocks (used/total), stats grid (8 metriche mem_*), swap I/O panel (5 metriche), CHECKPOINT/RESTORE buttons, form MEMW (pid + testo).
- `gui/widgets/orchestration.py` (~230 righe): JSON editor con template DAG, policy combo (fail_fast/best_effort), submit button, orchestration summary panel, task table 4 colonne (Task/Status/PID/Error), poll status.
- `gui/widgets/logs.py` aggiornato (~210 righe): terzo pane protocol trace (req→resp timestamped), export include protocol trace.
- `gui/app.py` aggiornato (~430 righe): wiring completo per processes (7 signals), memory (4 signals), orchestration (3 signals), _flush_ui_queue con 8 nuovi event kinds, _apply_status alimenta processes + memory sections.
- Testato offscreen: tutti i widget instantiation OK, STATUS parsing OK (memory bar 53%, proc table populated, orchestration task rows), protocol trace append OK, MainWindow wiring end-to-end OK.

**Copertura opcode completa:**
| Opcode | Sezione GUI |
|--------|-------------|
| PING | Chat (via protocol_client) |
| LOAD | Models (Load button) |
| EXEC | Chat (prompt submit) |
| KILL | Chat + Processes |
| TERM | Chat + Processes |
| STATUS | Sidebar + Processes + Memory + Orchestration |
| SHUTDOWN | Sidebar (Stop kernel) |
| MEMW | Memory (MEMW form) |
| LIST_MODELS | Models (auto-refresh) |
| SELECT_MODEL | Models (Select button) |
| MODEL_INFO | Models (Info button) |
| SET_GEN | Chat (gen params bar) |
| GET_GEN | Chat (gen params bar) |
| SET_PRIORITY | Processes (priority combo) |
| GET_QUOTA | Processes (quota panel) |
| SET_QUOTA | Processes (quota inputs) |
| CHECKPOINT | Memory (checkpoint button) |
| RESTORE | Memory (restore button) |
| ORCHESTRATE | Orchestration (JSON submit) |
| STATUS orch:N | Orchestration (poll status) |

---

## Fase 2.7: Risoluzione criticità (C1–C14)

### Criticità risolte
**Status:** Phase A (C12, C14, C4) `DONE` ✅ | Phase B (C1, C2, C6) `DONE` ✅ | Phase C (C3, C5, C7) `DONE` ✅ | Phase D (C8, C9, C10, C11, C13) `DONE` ✅

| Crit | Titolo | Note |
|------|--------|------|
| C1 | Inferenza bloccante nell'event loop | Inference worker thread + mpsc checkout/checkin |
| C2 | Incoerenza modello di concorrenza | Tutto in `Kernel` struct, `&mut` split borrows, no Arc/Mutex/Rc/RefCell |
| C3 | Nessuna autenticazione TCP | Token 32-byte hex al boot → `workspace/.kernel_token`, AUTH opcode, `AGENTIC_AUTH_DISABLED` env bypass |
| C4 | `execute_command` monolitico | Refactored in `commands/` submodules con `CommandContext` |
| C5 | STATUS flat → JSON strutturato | 8 `#[derive(Serialize)]` structs, GUI usa `json.loads()` ovunque, rimossi tutti `_ex()` regex helpers |
| C6 | Metriche globali statiche | `MetricsState` + `SyscallRateMap` come campi di `Kernel` |
| C7 | Qwen model reload per spawn | Guard in `spawn_process()`: reject concurrent spawn per backend non clonabili |
| C8 | GUI: connessione TCP nuova per ogni richiesta | `ProtocolClient` persistente con reconnect + lock |
| C9 | GUI: thread spawn per ogni richiesta | `ThreadPoolExecutor(max_workers=4)` per control-plane, thread dedicato solo per EXEC stream |
| C10 | GUI: HTML rebuild per ogni token di streaming | Render throttle 200 ms con `_render_dirty` in Chat |
| C11 | GUI: N+1 query Processes | STATUS globale include `active_processes`, tabella popolata senza richieste extra |
| C12 | Porta 6379 hardcoded | `AGENTIC_PORT` env var (default 6380) |
| C13 | Nessun recovery per swap worker thread crash | re-spawn singolo + counter `swap_worker_crashes` in STATUS |
| C14 | Debito tecnico minore | dead_code cleanup, lingua mista, unwrap → proper errors |
| C15 | Contratto `MEMW` incoerente | Canonical format `<pid>\n<raw-bytes>`, rifiuto payload disallineati, GUI Memory riallineata |
| C16 | `RESTORE` ambiguo | `metadata_only_clear_and_apply`, rifiuto su kernel busy, risposta JSON con limiti espliciti |
| C17 | Workload hint Chat non inoltrato | `capability=<hint>;` inoltrato solo fuori da `auto` |
| C18 | API modelli flat-text | `LIST_MODELS` e `MODEL_INFO` JSON, GUI e benchmark senza regex |
| C19 | Metriche Chat approssimate | marker finale con contatori reali scheduler, fallback `approx` esplicito |
| C20 | Contesto orchestrazione non bounded | cap `AGENTIC_ORCH_MAX_OUTPUT_CHARS`, marker `[TRUNCATED]`, contatori in `STATUS orch:` |
| C21 | Drift documentale/runtime | docs sincronizzate su auth, restore metadata-only, scheduler governance, focus local-first |

**Test:** 100/100 ✅ (96 precedenti + 4 nuove regressioni hardening)

**Esito fase critica:** le osservazioni della critica coerenti col prodotto sono state integrate in tre assi: contratti machine-readable, operator trust della GUI e bound espliciti sul runtime locale. Il progetto non apre per ora un cantiere Tokio/distributed.

---

## Roadmap attiva — Fase 2.8: AI Workstation OS hardening

### Decisione di prodotto

AgenticOS prosegue come **AI workstation OS local-first, single-node**.

Questo significa che il focus immediato e':
- massima correttezza del kernel locale e del control plane
- contratti protocollo/GUI solidi e machine-readable
- osservabilita' e restore onesti rispetto a cio' che il sistema puo' realmente fare
- disciplina del contesto e della memoria prima di introdurre feature agentiche piu' ambiziose

Non e' invece una priorita' immediata:
- rincorrere concorrenza forte o multi-worker generalizzato
- migrare ora a Tokio
- ampliare lo scope verso distribuzione o coordinazione remota multi-nodo

### 18) Protocollo, stato e documentazione coerenti
**Status:** `DONE` ✅

**Obiettivi**
- Chiudere i gap di correttezza tra protocollo, backend, GUI e documentazione.
- Rendere esplicite le semantiche reali di `MEMW` e `RESTORE`.
- Uniformare le API dei modelli al formato JSON gia' adottato da `STATUS`.

**DoD**
- [x] **18.1** `MEMW` ha un contratto unico, lossless e documentato; niente troncamento silenzioso.
- [x] **18.2** `RESTORE` e' transazionale sulla parte restore-able oppure rinominato/documentato in modo onesto.
- [x] **18.3** `LIST_MODELS` e `MODEL_INFO` restituiscono JSON strutturato.
- [x] **18.4** `ARCHITECTURE.md`, `ROADMAP.md`, `gui/README.md` riflettono auth, restore metadata-only e focus local-first.
- [x] **18.5** Suite test verde, clippy pulito o con warning residuali esplicitamente tracciati.

**Note di design**
- Questa milestone chiude C15, C16, C18 e C21 lato protocollo/documentazione.
- Lo scheduler va descritto per cio' che e' oggi: governance/ordering locale, non parallelismo forte.

**Esito**
- Adottati i punti coerenti della critica: contratti machine-readable, semantiche oneste e niente affordance ambigue tra GUI e kernel.

**Stima:** ~6-9h

---

### 19) GUI fidelity & operator trust
**Status:** `DONE` ✅

**Obiettivi**
- Far coincidere esattamente quello che la GUI promette con quello che il kernel esegue.
- Eliminare le affordance ingannevoli e le metriche non affidabili.
- Rafforzare la GUI come control center della workstation locale.

**DoD**
- [x] **19.1** Il workload hint della Chat (`auto/fast/code/reasoning/general`) viene davvero inoltrato al kernel.
- [x] **19.2** Le metriche Chat mostrano contatori reali per PID oppure sono marcate chiaramente come stime.
- [x] **19.3** La sezione Memory descrive `MEMW` come strumento low-level coerente con il backend.
- [x] **19.4** La sezione Models usa payload JSON e non parsing regex.
- [x] **19.5** Verifica Python-side aggiornata: `python -m compileall gui src/eval_swarm.py` verde e runbook GUI sincronizzato.

**Note di design**
- Questa milestone chiude C17 e C19, e completa il lato GUI di C15/C18.
- L'obiettivo non e' un redesign estetico ulteriore, ma fiducia operativa dell'utente.

**Esito**
- La GUI non promette piu' comportamenti inesistenti: hint inoltrati davvero, metriche finali reali, descrizioni Memory/Models coerenti col protocollo.

**Stima:** ~3-5h

---

### 20) Orchestration context discipline
**Status:** `DONE` ✅

**Obiettivi**
- Evitare che i DAG crescano senza bound nei buffer di output e nei prompt derivati.
- Preparare il terreno a un context management piu' sofisticato senza anticipare tutta la milestone futura.
- Rendere visibile in `STATUS orch:` quanta compaction/truncation viene applicata.

**DoD**
- [x] **20.1** Cap configurabile sull'output memorizzato per task/orchestrazione.
- [x] **20.2** Prompt dei task dipendenti costruiti con truncation/compaction esplicita.
- [x] **20.3** `STATUS orch:N` espone contatori di truncation/compression.
- [x] **20.4** Test su grafi con output voluminoso e dipendenze parallele.
- [x] **20.5** Suite test verde, clippy pulito.

**Note di design**
- Questa milestone chiude C20 e riduce il rischio prima della futura milestone di context window management.
- Il focus resta locale: bounds e disciplina prima di strategie agentiche piu' costose.

**Esito**
- Il runtime local-first non accumula piu' output orchestrati senza bound e rende visibili truncation e footprint del contesto all'operatore.

**Stima:** ~4-6h

---

## Roadmap attiva — Fase 2.9: Future-Model Flexibility

### Decisione architetturale

Qwen3.5 non va integrato come eccezione isolata: viene usato come pressione progettuale per rendere AgenticOS stabile nel tempo rispetto a famiglie, backend e metadata futuri.

Questo significa che il focus immediato e':
- separare il kernel dalla logica specifica del singolo backend modello
- trattare il runtime LLM come un driver intercambiabile, inclusi driver esterni futuri
- leggere a runtime metadata, tokenizer contract e chat template con fallback sensati
- sostituire il routing family-first con capability dichiarate e verificabili

Questo NON significa, in questa fase:
- aprire subito un cantiere multimodale completo
- riscrivere NeuralMemory per architetture non-Transformer
- cambiare il perimetro local-first/single-node

### 21) Model Abstraction Layer
**Status:** `DONE` ✅

**Obiettivi**
- Separare il kernel dal backend concreto di inferenza.
- Sostituire il coupling enum-based di `RuntimeModel` con un contratto stabile di backend.
- Preparare il supporto architetturale a driver interni ed esterni senza rompere Llama 3.1 e Qwen 2.5.

**DoD**
- [x] **21.1** Contratto backend introdotto (`ModelBackend` / wrapper `RuntimeModel`) in [src/backend.rs](src/backend.rs).
- [x] **21.2** Adattato il lifecycle per usare metadata del backend risolto in [src/engine/lifecycle.rs](src/engine/lifecycle.rs).
- [x] **21.3** Rimossi i path runtime/control-plane che dipendevano da una `PromptFamily` globale invece del contratto del modello/backend attivo.
- [x] **21.4** Esporre identificativo backend e vincoli runtime nel catalogo/info modello.
- [x] **21.5** Suite test verde, senza regressioni funzionali su Llama/Qwen legacy.

**Note di design**
- Questa milestone e' il prerequisito per C22 e blocca quasi tutto il resto del cantiere future-model-flexibility.
- Il primo slice implementato mantiene i backend Candle interni ma apre il perimetro per driver esterni successivi.

**Esito attuale**
- Avviato il disaccoppiamento del backend: `RuntimeModel` non e' piu' un enum chiuso; i driver interni Llama/Qwen2 sono stati adattati dietro un contratto trait-based.
- Il catalogo espone ora `resolved_backend`, stato del driver e razionale di risoluzione; `LLMEngine` carica il backend risolto invece di trattare `backend_preference` come dato passivo.
- `runtime.rs`, `commands/*`, `transport/io.rs` e il checkpoint flow non usano piu' `active_family` come pivot implicito: il runtime decide in base al modello/backend effettivamente caricato.
- `model_catalog.rs` espone ora un `ResolvedModelTarget` che centralizza family, metadata, tokenizer e driver resolution; `commands/model.rs`, `commands/exec.rs` e `engine/lifecycle.rs` non ricostruiscono piu' queste decisioni in modo disperso.
- Validazione aggiornata: `cargo test --release` verde a 112/112 dopo la centralizzazione del load target e del contratto di backend.

**Stima:** ~5-8h

---

### 22) Metadata-Driven Prompting & Tokenizer
**Status:** `DONE` ✅

**Obiettivi**
- Rendere metadata first-class nel catalogo modelli.
- Far dipendere template, special tokens e tokenizer contract da metadata runtime con fallback hardcoded solo per retrocompatibilita'.
- Preparare il supporto a sidecar metadata subito e parsing nativo GGUF/tokenizer config come step successivo.

**DoD**
- [x] **22.1** `ModelMetadata` introdotto nel catalogo con supporto a sidecar `metadata.json` / `<model>.metadata.json`.
- [x] **22.2** `LIST_MODELS` e `MODEL_INFO` espongono metadata source, backend preference e capability dichiarate.
- [x] **22.3** `prompting.rs` consuma chat template, assistant preamble e stop markers runtime quando disponibili.
- [x] **22.4** `engine/tokenizer.rs` consuma special tokens runtime quando disponibili.
- [x] **22.5** Fallback family-based esplicito e testato per modelli legacy privi di metadata.
- [x] **22.6** Parsing nativo da GGUF/tokenizer config introdotto in forma additiva, con overlay sidecar esplicito.

**Note di design**
- Partenza pragmatica: sidecar metadata prima, parsing nativo da GGUF/tokenizer config dopo.
- Il percorso deve essere additive e non rompere i modelli gia' presenti nel repository.

**Esito attuale**
- `model_catalog.rs` unisce ora metadata nativi e sidecar: legge `general.architecture` e `tokenizer.chat_template` dal GGUF quando disponibili, estrae token speciali e stop markers dal `tokenizer.json`, e applica il sidecar solo come overlay esplicito.
- Il catalogo espone `metadata_source` distinguendo fonti native (`gguf`, `tokenizer`) e sidecar, senza rompere i path legacy gia' introdotti in engine/runtime.
- Validazione aggiornata: `cargo test --release` verde a 112/112, inclusi i nuovi test sul parsing nativo del catalogo.

**Stima:** ~6-9h

---

### 23) Capability Routing v2
**Status:** `DONE` ✅

**Obiettivi**
- Spostare il routing da precedenze statiche per famiglia a capability dichiarate dai modelli.
- Mantenere euristiche statiche solo come fallback per modelli legacy.
- Rendere osservabile nella GUI il motivo della selezione modello.

**DoD**
- [x] **23.1** `select_for_workload()` usa capability dichiarate come prima fonte, con fallback alle euristiche family-based.
- [x] **23.2** `routing_recommendations` espone chiaramente il path capability-driven vs fallback.
- [x] **23.3** GUI Models mostra capability, metadata source, backend preference e razionale del routing quando presenti.
- [x] **23.4** Test unitari su modelli con e senza metadata.

**Note di design**
- Questa milestone chiude il gap C25 solo quando il fallback legacy resta esplicito, stabile e testato.

**Esito attuale**
- Routing capability-driven completato nella prima forma operativa: il catalogo espone `source`, `rationale`, `capability_key`, `capability_score`, e la GUI rende osservabile il perche' della selezione.

**Stima:** ~4-6h

---

### 24) External Driver Plane
**Status:** `DONE` ✅

**Obiettivi**
- Introdurre una separazione netta tra control plane del kernel e driver plane del modello.
- Predisporre il supporto a driver esterni senza imporre subito una dipendenza produttiva completa.
- Definire registry e policy di risoluzione modello -> driver.

**DoD**
- [x] **24.1** Registry dei driver con capability minime e identificativo backend.
- [x] **24.2** Policy di risoluzione modello -> driver basata su metadata e requisiti runtime.
- [x] **24.3** Supporto almeno a un driver esterno mock/stub per validare la separazione architetturale.
- [x] **24.4** Errori espliciti quando nessun driver soddisfa i requisiti del modello.

**Esito attuale**
- Introdotto un driver registry esplicito con backend interni loadable e uno stub esterno `external-llamacpp`.
- La risoluzione modello -> driver e' ora parte del control plane: `backend_preference` puo' essere soddisfatta, fallbackata o rifiutata con razionale machine-readable.
- `LIST_MODELS` e `MODEL_INFO` espongono `resolved_backend`, stato del driver e ragione della risoluzione.

**Stima:** ~6-10h

---

### 25) Qwen3.5 Integration Pilot
**Status:** `DONE` ✅

**Obiettivi**
- Usare Qwen3.5 come primo caso guida del nuovo strato model-agnostic.
- Validare che il design regga backend/metadata piu' complessi senza reintrodurre coupling nel kernel.
- Produrre una checklist concreta per integrare famiglie future con costo marginale basso.

**DoD**
- [x] **25.1** Matrice compatibilita' Qwen3.5: tokenizer, template, special tokens, driver richiesto, limiti runtime.
- [x] **25.2** Caricamento e discovery coerenti col nuovo strato metadata/driver.
- [x] **25.3** Nessun fix Qwen3.5-specific deve violare il contratto generale del driver layer.
- [x] **25.4** Aggiornamento documentale in `ARCHITECTURE.md` con i nuovi contratti.

**Esito 2026-03-08**
- Il catalogo scopre `models/qwen3.5-9b/Qwen3.5-9B-Q4_K_M.gguf`, risolve il `tokenizer.json` locale e conserva `general.architecture=qwen35` come metadata first-class.
- `LIST_MODELS` e `MODEL_INFO` espongono ora anche `architecture`, oltre a `resolved_backend`, `driver_resolution_source` e `driver_resolution_rationale`.
- La policy modello -> driver e' diventata architecture-aware: `PromptFamily::Qwen` non implica piu' fallback automatico verso `candle.quantized_qwen2` quando il GGUF dichiara `qwen35`.
- Il `LOAD` di Qwen3.5 viene rifiutato in modo esplicito e machine-readable prima del backend load, perche' Candle 0.9.1 nel repo supporta solo `quantized_qwen2`; il prossimo passo funzionale e' aggiungere un driver compatibile (`qwen35` o esterno), non un branch speciale nel kernel.
- Validazione: `cargo test` = 114 passed, 1 ignored; smoke test locale su `models/qwen3.5-9b` verde per discovery/tokenizer/architecture-aware rejection.

**Stima:** ~4-8h

---

## Roadmap attiva — Fase 2.10: Microkernel Driver Boundary

### Decisione architetturale

AgenticOS introduce ora un boundary microkernel esplicito tra kernel Rust, driver di inferenza e meccanismo fisico della memoria di contesto.

Questo significa che il focus immediato e':
- mantenere il kernel 100% Rust e isolare i backend non-Rust in processi separati
- sostituire l'accoppiamento diretto `NeuralMemory` <-> tensori Candle con una nozione astratta di context slot
- spostare il meccanismo fisico di save/load/free del contesto dentro i driver, lasciando nel kernel solo policy, accounting e safety
- preparare un driver RPC compatibile con `llama.cpp` per architetture non supportate nativamente da Candle, incluso `qwen35`

Questo NON significa, in questa fase:
- introdurre FFI C/C++ nel kernel
- demandare al driver esterno le policy di path safety, quota o lifecycle del processo
- vincolare il design a `llama-server` puro se servira' un adapter RPC controllato da AgenticOS

### 26) Astrazione NeuralMemory (Context Slots)
**Status:** `DONE` ✅

**Obiettivi**
- Disaccoppiare `src/memory/core.rs` dai `Tensor` di Candle e dalla nozione di storage fisico in-process.
- Rendere `NeuralMemory` un manager logico di context slots, quote, LRU e scheduling dello swap.
- Delegare il meccanismo materiale di save/load/free al backend attivo tramite un contratto esplicito.

**DoD**
- [x] Evolvere il contratto backend separando chiaramente inferenza e context-slot persistence, evitando di modellare il driver RPC come semplice backend Candle-like.
- [x] Introdurre metodi backend per `save/load/free_context_slot` con semantica esplicita e supporto a errori `unsupported` quando il backend non implementa ancora persistence.
- [x] Rinominare e riallineare i tipi memory-side da `TensorId`/page ownership a una nozione astratta di `ContextSlotId` o equivalente, separata dal PID.
- [x] `NeuralMemory` mantiene policy, accounting, waiting state, quota, LRU e swap orchestration; non possiede piu' il meccanismo fisico dei tensori del backend.
- [x] La path safety e l'allocazione della destinazione di swap restano kernel-side; il driver riceve solo destinazioni gia' validate o handle equivalenti.
- [x] I backend Candle assorbono il meccanismo fisico oggi accoppiato a `swap_io.rs` senza perdere le garanzie esistenti di atomicita' e sicurezza.
- [x] I test esistenti passano senza regressioni, con copertura aggiuntiva sul nuovo boundary policy/meccanismo.

**Note di design**
- Non aggiungere questi metodi in modo ingenuo all'attuale trait `ModelBackend` se questo preserva il coupling `Tensor -> Tensor`; il contratto va rifinito per accomodare backend RPC e backend in-process con semantiche diverse.
- `pid` puo' restare input operativo nel primo slice, ma l'obiettivo del design e' una risorsa distinta di memoria persistibile (`ContextSlotId`) non coincidente per forza con il lifecycle del processo.
- Il kernel continua a essere la fonte di verita' per quota, eviction, LRU e validazione dei path sotto `workspace/`.

**Esito parziale 2026-03-08**
- Introdotto `ContextSlotId` come astrazione transizionale nel sottosistema memoria e nel boundary backend.
- `NeuralMemory` traccia ora internamente `slot_table`, `pid_to_slot`, owner dei context slot e LRU per slot, mantenendo invariati i contratti esterni correnti.
- Aggiunti hook backend `save/load/free_context_slot` come boundary esplicito tra policy kernel-side e meccanismo driver-side; per ora i backend Candle rispondono con `unsupported` finche' il meccanismo fisico non viene migrato.
- Lo swap async prepara ora target validati slot-aware nel kernel (`pid` + `slot`) prima dell'enqueue del worker; la path policy resta nel kernel e il worker esegue solo la persistenza atomica del target ricevuto.
- Il lifecycle di cleanup dei PID usa ora `free_context_slot` quando conosce uno slot logico; il boundary backend non e' piu' solo dichiarativo e il rilascio slot entra nei path reali di runtime e comandi TERM/KILL.
- La persistenza fisica raw-bytes per il path Candle e' stata spostata da `swap_io.rs` al dispatch backend-aware in `src/backend.rs`; `swap_io.rs` resta responsabile solo della preparazione sicura dei target.

**Esito finale 2026-03-08**
- `NeuralMemory` e' ora un manager logico di context slots: non possiede piu' `Tensor`, `Device` o blocchi fisici Candle e mantiene solo admission policy, quota, LRU, waiting state, block accounting e orchestration dello swap.
- Il meccanismo fisico di persistence per il path Candle raw-compat e' stato spostato dietro dispatch backend-aware in `src/backend.rs`; `swap_io.rs` prepara solo target validati sotto `workspace/`.
- `MEMW`, swap async e cleanup PID usano tutti il boundary a slot (`ContextSlotId`) e i path runtime/control-plane invocano `free_context_slot` quando noto.
- Validazione finale: `cargo test` verde a `116 passed, 1 ignored`.

### 27) Driver Esterno RPC (llama.cpp)
**Status:** `DONE` ✅

**Obiettivi**
- Implementare un driver esterno RPC, compatibile con `llama.cpp`, per supportare architetture non native nel runtime Candle.
- Mantenere isolamento di fault: crash, OOM o segfault del backend esterno non devono compromettere il kernel.
- Instradare modelli come `qwen35` verso questo piano driver senza introdurre branch speciali nel kernel.

**DoD**
- [x] Introdurre un backend/driver RPC separato dal path in-process, con contratto compatibile con inferenza remota e gestione dei context slot.
- [x] Implementare generation step/chunk tramite chiamate RPC locali HTTP verso un server esterno compatibile con `llama.cpp`.
- [x] Implementare `save/load/free_context_slot` tramite RPC equivalenti verso il processo driver esterno o un adapter dedicato.
- [x] Gestire timeout, errori di trasporto e server non disponibile in modo graceful, senza bloccare l'event loop `mio` e senza far crashare il kernel.
- [x] Aggiornare `DRIVER_REGISTRY` affinche' architetture non supportate da Candle possano risolversi verso il driver RPC quando disponibile.
- [x] Documentare esplicitamente il boundary: policy nel kernel, meccanismo nel driver, nessuna FFI C/C++ nel kernel Rust.

**Note di design**
- HTTP/REST locale e' accettabile come primo transport, ma va eseguito fuori dal loop `mio` tramite worker/driver plane dedicato; il beneficio non viene dal protocollo in se', ma dall'isolamento del processo e dal disaccoppiamento del control plane.
- Il target iniziale puo' essere `llama-server`, ma il design deve ammettere un piccolo adapter RPC se le API pubbliche non coprono in modo pulito session persistence o slot management.
- L'attivazione runtime del driver esterno e' condizionata alla presenza di `AGENTIC_LLAMACPP_ENDPOINT`; in assenza dell'endpoint il fallback resta sui driver Candle loadable e architetture come `qwen35` restano `DRIVER_UNRESOLVED`.

**Esito finale 2026-03-08**
- Il contratto di inferenza e' stato riallineato a `generate_step(...)`: il worker non assume piu' un backend locale `forward(Tensor) -> Tensor`, quindi un backend RPC puo' produrre testo/token senza branch speciali nel runtime.
- `external-llamacpp` e' ora un backend reale: usa HTTP locale verso le route effettive di `llama-server` (`POST /completion` con `id_slot`, `return_tokens`, `cache_prompt`; `POST /slots/{id}?action=save|restore|erase`), con timeout configurabile via `AGENTIC_LLAMACPP_TIMEOUT_MS`.
- La risoluzione driver e' runtime-aware: quando `AGENTIC_LLAMACPP_ENDPOINT` e' configurato, architetture non supportate nativamente da Candle possono risolversi verso il piano RPC esterno.
- Operativamente il server `llama.cpp` deve esporre gli endpoint slot e avere una `--slot-save-path` coerente con la strategia di persistence usata dal kernel.
- Aggiunto opcode diagnostico `BACKEND_DIAG` per interrogare dal kernel lo stato del backend esterno (`/health`, `/props`, `/slots`) e restituire un report JSON machine-readable utile al control plane.
- Validazione finale: `cargo test` verde a `119 passed, 1 ignored`, inclusi test mock per generation RPC, slot RPC e backend diagnostics.

---

## Roadmap futura — Fase 3: Intelligenza agentica

### 28) Tool registry dinamico
**Status:** `TODO`

**Obiettivi**
- Sostituire il registry hardcoded di tool (`python`, `write_file`, `calc`) con un sistema dinamico.
- Permettere registrazione/discovery di tool a runtime da parte di agenti o plugin esterni.
- Abilitare estensibilità senza modificare il codice kernel.

**DoD**
- [ ] **28.1** `ToolDescriptor` struct: nome, descrizione, schema input (JSON Schema), backend (host/wasm/remote).
- [ ] **28.2** `ToolRegistry` con `register(descriptor)` / `unregister(name)` / `list()` / `resolve(name)`.
- [ ] **28.3** Opcodes protocollo: `REGISTER_TOOL <json>`, `LIST_TOOLS`, `TOOL_INFO <name>`.
- [ ] **28.4** Dispatch syscall aggiornato: lookup nel registry prima del dispatch hardcoded.
- [ ] **28.5** Tool remoto: possibilità di registrare un tool che fa HTTP call a un endpoint esterno.
- [ ] **28.6** Integrazione GUI: pannello tool con lista, dettagli, register/unregister.
- [ ] **28.7** Test: register + invoke, unregister + fallback, tool remoto mock.
- [ ] **28.8** Suite test verde, clippy pulito.

**Note di design**
- Il registry è in-memory (non persistente nel primo rilascio). Checkpoint/restore (M14) può serializzarlo in futuro.
- I tool WASM sono già supportati come backend sandbox — il registry li rende discoverable programmaticamente.
- Il JSON Schema dei tool è compatibile con il formato OpenAI function calling per interoperabilità.

**Stima:** ~4-6h

---

### 29) Context window management
**Status:** `TODO`

**Obiettivi**
- Gestire intelligentemente la finestra di contesto LLM per processi long-running.
- Evitare troncamento silenzioso su prompt lunghi e conversazioni multi-turn.
- Abilitare strategie di compressione contesto (summarization, sliding window, retrieval).

**DoD**
- [ ] **29.1** Tracking context usage: ogni processo traccia token count corrente vs window size del modello caricato.
- [ ] **29.2** Strategia `sliding_window`: mantiene ultimi N token, scarta i più vecchi con boundary allineato a turni conversazione.
- [ ] **29.3** Strategia `summarize`: quando il contesto supera soglia, genera un riassunto dei turni più vecchi usando il modello stesso e lo sostituisce al contesto originale.
- [ ] **29.4** Strategia `retrieve`: integrazione con NeuralMemory per storage/retrieval semantico di frammenti di contesto evicted (RAG-like from memory store).
- [ ] **29.5** Strategia selezionabile per processo via hint su `EXEC` (`context_strategy=sliding|summarize|retrieve`).
- [ ] **29.6** Metriche context in `STATUS`: `context_tokens_used`, `context_window_size`, `context_compressions`.
- [ ] **29.7** Test unitari: overflow detection, sliding window boundary, summarize trigger.
- [ ] **29.8** Suite test verde, clippy pulito.

**Note di design**
- La strategia `summarize` introduce un "meta-step" nel runtime: il processo genera un riassunto prima di proseguire. Richiede attenzione al budget token (la summarization stessa consuma quota).
- La strategia `retrieve` sfrutta NeuralMemory come store vettoriale — i tensori già allocati per PID diventano embedding recuperabili. Questo è il primo passo verso una vera memoria episodica.
- Default: `sliding_window` (zero overhead). Le altre strategie sono opt-in.

**Stima:** ~6-10h

---

## Sequenza di esecuzione prevista

```
Fase 2 — Consolidamento + Orchestrazione (marzo 2026)
  ├─ M13  Swap extraction            ~1-2h    (debito tecnico)
  ├─ M14  Persistence & Snapshot     ~6-8h    (infrastruttura critica)
  ├─ M15  ARCHITECTURE.md            ~2-3h    (documentazione)
  ├─ M16  Agent orchestration        ~4-6h    (feature strategica)
  └─ M17  Benchmark swarm            ~2-3h    (validazione) ✅

Fase 2.5 — GUI Redesign (marzo 2026)
  ├─ M10.1 Refactor strutturale      ~8-10h   (sidebar + Chat + Models + Logs + theme)
  └─ M10.2 Sezioni avanzate          ~6-8h    (Processes + Memory + Orchestration)

Fase 2.8 — AI Workstation OS hardening (marzo 2026)
  ├─ M18  Protocollo & stato coerenti    ~6-9h   (correttezza local-first)
  ├─ M19  GUI fidelity & trust           ~3-5h   (GUI coerente col kernel)
  └─ M20  Orchestration discipline       ~4-6h   (bound contesto prima di M22)

Fase 2.9 — Future-Model Flexibility (marzo 2026)
  ├─ M21  Model Abstraction Layer        ~5-8h   (backend trait + disaccoppiamento runtime)
  ├─ M22  Metadata-driven prompting      ~6-9h   (sidecar ora, parsing nativo dopo)
  ├─ M23  Capability routing v2          ~4-6h   (routing dichiarativo + fallback legacy)
  ├─ M24  External driver plane          ~6-10h  (registry e model-driver resolution)
  └─ M25  Qwen3.5 integration pilot      ~4-8h   (caso guida, non eccezione)

Fase 2.10 — Microkernel Driver Boundary (marzo 2026)
  ├─ M26  Astrazione NeuralMemory        ~6-10h  (context slots + boundary policy/meccanismo)
  └─ M27  Driver esterno RPC             ~6-10h  (llama.cpp compatibile, no FFI nel kernel)

Fase 3 — Intelligenza agentica (aprile 2026)
  ├─ M28  Tool registry dinamico         ~4-6h   (estensibilità)
  └─ M29  Context window mgmt            ~6-10h  (qualità agentica)
```

---

## Registro avanzamento

> Le entry dettagliate delle milestone 1–12 sono archiviate qui in forma compressa.
> Per il log completo delle singole sub-task, consultare la history git.

| Data | Milestone | Stato | Note sintetica |
|------|-----------|-------|----------------|
| 2026-02-22 | M1 | DONE | Runtime stabile: flush I/O, cleanup PID, marker PROCESS_FINISHED |
| 2026-02-22 | M2 | DONE | Protocollo hardened: header strict, reply codificate, framing DATA |
| 2026-02-22 | M3 | DONE | Inferenza calibrata: stop policy per-family, GET_GEN/SET_GEN, benchmark baseline |
| 2026-02-22 | M5 | DONE | Test professionali: multi-client, reconnect, benchmark commit-aware |
| 2026-02-23 | M4 | DONE | SysCall hardened: timeout, path safety, audit log, sandbox mode, rate-limit |
| 2026-02-23 | M6 | DONE | Osservabilità: STATUS/TERM/KILL/SHUTDOWN, metriche, logging strutturato, graceful shutdown |
| 2026-02-23 | M7 | DONE | Multi-LLM: catalogo auto-discovery, backend Qwen, capability scheduler v1, 27 test |
| 2026-02-23 | M8 | DONE | NeuralMemory: ownership PID, quote token-slot, swap asincrono, eviction LRU, 37 test |
| 2026-02-23 | M10 | DONE | GUI desktop PySide6: controllo kernel, EXEC streaming, modelli, osservabilità, runbook |
| 2026-03-04 | M11 | DONE | Refactoring qualità: config.rs, estrazione funzioni runtime, memory/types.rs, 54 test |
| 2026-03-04 | M12 | DONE | Hardening: thiserror error hierarchy, tracing logging, struct Kernel, eviction.rs, CI |
| 2026-03-04 | M12.1 | DONE | Migrazione errori: CatalogError, ProtocolError, EngineError::Backend, 54 test |
| 2026-03-04 | M9 | DONE | Scheduler: ProcessPriority, ProcessQuota, enforcement runtime, 3 nuovi opcodes, 67 test |
| 2026-03-05 | M13 | DONE | Swap extraction: `memory/swap.rs` (236 righe) con `SwapManager`. `core.rs` 809→673 (424 prod). Suite invariata 67/67, clippy pulito. |
| 2026-03-05 | M14 | DONE | Persistence: `checkpoint.rs` (340 righe) con KernelSnapshot + 6 sub-types. Opcodes CHECKPOINT/RESTORE. Auto-checkpoint timer. 74 test, clippy pulito. |
| 2026-03-05 | M15 | DONE | ARCHITECTURE.md (~500 righe): 14 sezioni, 8 diagrammi Mermaid, tabella 18 opcodes, glossario 20 termini. |
| 2026-03-05 | M16 | DONE | Orchestrator: `orchestrator.rs` (~500 righe), opcode ORCHESTRATE, DAG validation + advance, fail_fast/best_effort, STATUS orch:, 94 test, clippy pulito. |
| 2026-03-05 | M17 | DONE | Benchmark swarm: `eval_swarm.py` (520 righe), report JSON. Fix OOM con drop-before-load in LOAD+EXEC handlers. Scenario A (Llama 8B) funzionante, scenario B (swarm) skippato per limiti HW (31 GB insufficienti per Qwen 14B swap). |
| 2026-03-06 | C1 | DONE | Non-blocking inference: checkout/checkin pattern in `inference_worker.rs`. Forward pass offloaded a thread dedicato via `mpsc` channels. Event loop non più bloccato. 94/94 test, zero warnings nuovi. |
| 2026-03-06 | C2 | DONE | Concurrency model unificato: `Arc<Mutex<Option<LLMEngine>>>` → `Option<LLMEngine>`, `Rc<RefCell<NeuralMemory>>` → owned `NeuralMemory`. Tutto lo stato owned da `Kernel`, passato via `&mut` split borrows (NLL). Zero overhead da interior mutability. 16 file modificati, 94/94 test. |
| 2026-03-06 | C6 | DONE | Metriche de-static: `MetricsState` da `static OnceLock<Mutex>` a campo Kernel (via `CommandContext.metrics`). `RATE_STATES` da static a `SyscallRateMap` campo Kernel (via `run_engine_tick`). `started_at: Instant` in `MetricsState`. Test isolati con istanze locali. 94/94 test. |
| 2026-03-07 | AD1 | DECIDED | AgenticOS confermato come AI workstation OS local-first single-node. Priorita' immediata: chiudere C15-C21. `mio` confermato, Tokio rinviato finche' non emergono requisiti reali di concorrenza forte. |
| 2026-03-07 | M18 | DONE | Protocollo e stato riallineati: `MEMW` canonico e lossless, `RESTORE` metadata-only clear+apply, model APIs JSON, docs sincronizzate. |
| 2026-03-07 | M19 | DONE | GUI fidelity: workload hint realmente inoltrato, metriche Chat reali o `approx`, Memory/Models coerenti col protocollo. |
| 2026-03-07 | M20 | DONE | Orchestration discipline: cap configurabile, marker `[TRUNCATED]`, contatori di truncation/output in `STATUS orch:` e GUI. |
| 2026-03-07 | M21 | IN_PROGRESS | Avviato il Model Abstraction Layer: `RuntimeModel` non piu' enum chiuso, backend interni Llama/Qwen2 adattati dietro trait, lifecycle allineato ai metadata del backend. Catalogo/info modello espongono backend risolto e vincoli runtime. Test verdi 109/109. |
| 2026-03-07 | M22 | IN_PROGRESS | Metadata runtime-first end-to-end: sidecar nel catalogo, `LLMEngine` carica metadata, `prompting.rs` e `engine/tokenizer.rs` consumano template/stop markers/special tokens con fallback legacy testato. |
| 2026-03-07 | M23 | DONE | Capability routing v2 prima tranche chiusa: routing recommendations spiegabili (`source`, `rationale`, score), GUI Models aggiornata per capability/backend/metadata source, test verdi 105/105. |
| 2026-03-07 | M24 | DONE | Driver plane prima tranche chiusa: registry driver esplicito, risoluzione modello -> driver, stub esterno `external-llamacpp`, errori chiari su driver non risolvibili. Test verdi 109/109. |
| 2026-03-08 | M25 | DONE | Pilot Qwen3.5 chiuso lato control plane: discovery, tokenizer locale, `architecture=qwen35`, driver resolution architecture-aware, `LOAD` rifiutato con `DRIVER_UNRESOLVED` finche' non esiste un driver compatibile. `cargo test`: 114 passed, 1 ignored. |
| 2026-03-08 | M26 | DONE | `NeuralMemory` riallineata a context slots: niente piu' storage fisico Candle nel kernel, swap target kernel-side validati, persistence raw Candle dispatchata via backend, cleanup PID collegato a `free_context_slot`. `cargo test`: 116 passed, 1 ignored. |
| 2026-03-08 | M27 | DONE | Driver RPC esterno reale: contratto `generate_step(...)`, backend `external-llamacpp` via HTTP locale con timeout e RPC `save/load/free_context_slot`, risoluzione runtime-aware per architetture non supportate da Candle, opcode `BACKEND_DIAG` per `/health` `/props` `/slots`. `cargo test`: 119 passed, 1 ignored. |

---

## Nota architetturale — Migrazione a Tokio

### Decisione attuale
Il kernel resta su **mio + thread dedicato** (checkout/checkin pattern per l'inferenza).

La decisione di prodotto del 2026-03-07 e' che AgenticOS evolve prima come **AI workstation OS local-first, single-node**. Di conseguenza Tokio e' ancora fuori scope nel breve termine: prima si chiudono correttezza, contratti e affidabilita' locale; solo dopo si rivaluta un eventuale salto verso concorrenza forte.

Tokio e' stato valutato ma scartato per costo/beneficio:

- **Costo Tokio**: ~40-50% del codebase Rust da riscrivere (event loop mio → tokio runtime, `NeuralMemory` owned → `Arc<Mutex<>>` o `Arc<RwLock<>>`, tutti i 94 test da adattare). Stima: ~1-2 settimane. Con C2 completato (niente più `Rc<RefCell>`), la migrazione è più semplice.
- **Costo checkout/checkin**: ~1 giorno. Zero dipendenze nuove, zero test rotti.

### Quando rivalutare Tokio
Il trigger è uno di questi scenari:
1. **3+ worker thread indipendenti** — se servono tool remoti (M28), summarize meta-step (M29), driver RPC esterni persistenti o multi-model inference in parallelo, la coordinazione manuale con thread + mpsc diventa fragile. Tokio offre `select!`/`join!` nativi.
2. **Kernel distribuito** — se servono nodi remoti, gRPC, o comunicazione inter-kernel, Tokio + tonic diventano lo stack naturale.
3. **Backpressure complesso** — se il worker thread si satura e serve flow control sofisticato (bounded channels con politiche di drop/retry), Tokio offre primitivi migliori.

### Benefici a lungo termine di Tokio
- **Composizione async nativa**: `select!` per multiplexare canali, timer, I/O senza polling manuale
- **Per-connection tasks**: ogni client gestito da un task leggero (spawn per connection)
- **Backpressure**: `tokio::sync::mpsc` con bounded channels e `Permit`
- **Ecosistema**: tonic (gRPC), reqwest, tower (middleware), tokio-console (debug live)
- **Cancellation**: CancellationToken per shutdown gerarchico pulito

### Cosa NON cambia con Tokio
- La forward pass del modello (~150-250ms) è **CPU-bound** — resta in `spawn_blocking()` anche con Tokio
- Il vantaggio di Tokio è per l'I/O e la coordinazione, non per il calcolo

---

## Template aggiornamento (copia/incolla)

```md
### X) Titolo punto
**Status:** `IN_PROGRESS`

**Obiettivi**
- ...

**DoD**
- [ ] ...
- [ ] ...

**Esito**
- ...

**Evidenze**
- file: ...
- test/command: ...
```
