# AgenticOS — Roadmap Operativa

Questo file è la fonte unica di verità per il piano del progetto.

## Come usarla

- Aggiornare lo stato di ogni punto a fine attività (`TODO` → `IN_PROGRESS` → `DONE`).
- Registrare data, note sintetiche e commit/riferimenti utili.
- Non aprire un nuovo punto senza una **Definition of Done (DoD)** verificabile.

---

## Stato attuale (snapshot)

- **Data snapshot:** 2026-03-05
- **Versione:** `v0.5.0`
- **Runtime:** server TCP event-driven (`mio 1.0`) + engine LLM process-centric (`candle 0.9`)
- **Codebase:** ~5.000 righe Rust (kernel) + ~1.300 righe Python (GUI PySide6)
- **Test suite:** 67 test verdi (`cargo test --release`), clippy pulito, CI GitHub Actions
- **Modelli supportati:** Llama 3.1 (Q4_K_M) + Qwen 2.5 (Q4_K_M) con auto-discovery e capability routing
- **Dipendenze chiave:** `candle-core/candle-transformers 0.9.1`, `tokenizers 0.22.2`, `mio 1.0`, `thiserror 2.0`, `tracing 0.1`

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

**Totale test accumulati:** 37 (fine M8) → 54 (fine M11/M12) → 67 (fine M9-scheduler)

---

## Mappa codebase corrente

```
src/
├── main.rs              # struct Kernel, event loop mio, bootstrap (~215 righe)
├── protocol.rs          # OpCode enum, CommandHeader parser, response formatting
├── config.rs            # env_bool, env_u64, env_usize centralizzati
├── errors.rs            # KernelError, MemoryError, EngineError, ProtocolError, CatalogError
├── model_catalog.rs     # ModelCatalog auto-discovery, WorkloadClass, capability routing
├── prompting.rs         # PromptFamily (Llama/Qwen/Mistral), system/user templates, stop policy
├── backend.rs           # RuntimeModel trait, dispatch per famiglia (quantized_llama/qwen2)
├── process.rs           # AgentProcess, ProcessState, SamplingParams
├── runtime.rs           # run_engine_tick, dispatch_process_syscall, scan_syscall_buffer
├── scheduler.rs         # ProcessScheduler, ProcessPriority, ProcessQuota, ResourceAccounting
├── tools.rs             # Syscall sandbox (python/write_file/calc), rate-limit, audit
├── commands/
│   ├── mod.rs           # execute_command dispatch (~520 righe)
│   ├── metrics.rs       # StatusMetrics, formattazione STATUS
│   └── parsing.rs       # parse_generation_payload, parse_memw_payload
├── engine/
│   ├── mod.rs           # LLMEngine, spawn/step/kill process
│   ├── lifecycle.rs     # load_engine_from_catalog
│   └── tokenizer.rs     # load_tokenizer, validate_chat_template
├── memory/
│   ├── mod.rs           # re-export
│   ├── types.rs         # TensorId, MemorySnapshot, SwapEvent, MemoryConfig
│   ├── core.rs          # NeuralMemory allocatore (~560 righe)
│   ├── eviction.rs      # LRU eviction (clear/touch/victim/evict_until_fit)
│   └── swap_io.rs       # Swap I/O worker, path validation, atomic write
└── transport/
    ├── mod.rs           # re-export + test integration TCP (~510 righe)
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
**Status:** `TODO`

**Obiettivi**
- Estrarre la logica swap worker da `memory/core.rs` in modulo dedicato `memory/swap.rs`.
- Ridurre `core.rs` sotto le 450 righe, migliorando manutenibilità.
- Nessun cambio funzionale: puro refactoring strutturale.

**DoD**
- [ ] Modulo `memory/swap.rs` creato con: `SwapWorker`, `SwapQueue`, metodi `enqueue_swap`, `poll_completions`, `drain`.
- [ ] `memory/core.rs` sotto 450 righe, con `use swap::*` per delegare.
- [ ] `memory/mod.rs` aggiornato con re-export.
- [ ] Suite test invariata e verde (67/67).
- [ ] Clippy pulito.

**Stima:** ~1-2h

---

### 14) Persistence & Snapshot — checkpoint/restore stato kernel
**Status:** `TODO`

**Obiettivi**
- Permettere salvataggio periodico dello stato kernel su disco (checkpoint).
- Permettere restore dello stato al boot (processi attivi, scheduler, metriche).
- Garantire che un crash non perda l'intero contesto di lavoro.

**DoD**
- [ ] **14.1** Definire `KernelSnapshot` serializzabile (`serde`) con: processi attivi (PID, stato, prompt, workload), scheduler state (priorità, quote, accounting), metriche correnti.
- [ ] **14.2** Comando protocollo `CHECKPOINT` che scrive snapshot su `workspace/checkpoint.json` (atomic write via temp+rename).
- [ ] **14.3** Checkpoint automatico periodico configurabile via `AGENTIC_CHECKPOINT_INTERVAL_SECS` (default: disabilitato, 0).
- [ ] **14.4** `Kernel::restore_from(path)` che al boot ricarica stato da ultimo checkpoint valido. Processi non ripristinabili marcati come `Orphaned` con log warning.
- [ ] **14.5** Comando protocollo `RESTORE` per trigger manuale.
- [ ] **14.6** Test unitari: serializzazione/deserializzazione roundtrip, checkpoint atomico, restore con dati corrotti (graceful fallback).
- [ ] **14.7** Suite test verde, clippy pulito.

**Dipendenze nuove:** `serde 1.0`, `serde_json 1.0`

**Rischi & mitigazioni**
- Il restore non può ricaricare pesi LLM in RAM → i processi vengono segnalati come `Orphaned` e richiedono un nuovo `LOAD` + re-EXEC.
- Lo swap state su disco è già persistente → il checkpoint deve solo registrare i riferimenti, non duplicare dati.

**Stima:** ~6-8h

---

### 15) Documentazione architetturale — ARCHITECTURE.md
**Status:** `TODO`

**Obiettivi**
- Creare un documento di design che descriva l'architettura complessiva del sistema.
- Rendere il progetto comprensibile a contributor esterni o a sé stessi tra 6 mesi.
- Includere diagrammi a blocchi e flussi end-to-end.

**DoD**
- [ ] **15.1** `ARCHITECTURE.md` nella root con: overview sistema, diagramma a blocchi (kernel, engine, memory, scheduler, transport, tools), glossario concetti chiave.
- [ ] **15.2** Flusso end-to-end documentato: `LOAD` → `EXEC` → token generation → syscall dispatch → `PROCESS_FINISHED`.
- [ ] **15.3** Sezione "Memory subsystem" con diagramma alloc/eviction/swap lifecycle.
- [ ] **15.4** Sezione "Scheduler" con diagramma priority ordering + quota enforcement.
- [ ] **15.5** Sezione "Protocol" con tabella completa opcodes, formato header/reply, esempi.
- [ ] **15.6** Diagrammi in Mermaid (renderizzabili su GitHub).

**Stima:** ~2-3h

---

### 16) Agent orchestration primitives — task graph
**Status:** `TODO`

**Obiettivi**
- Introdurre primitive per orchestrazione multi-agente: un processo "orchestratore" lancia sub-task, raccoglie risultati, decide il passo successivo.
- Abilitare workflow strutturati (DAG di task) e non solo esecuzioni isolate.
- Mantenere l'approccio event-driven senza blocking nel loop principale.

**DoD**
- [ ] **16.1** Tipo `TaskGraph` con nodi (task con prompt + workload hint + dipendenze) e archi (data flow).
- [ ] **16.2** Opcode `ORCHESTRATE <json_task_graph>` che registra un grafo, spawna il primo livello di task indipendenti, e avanza automaticamente quando le dipendenze sono soddisfatte.
- [ ] **16.3** Stato orchestrazione interrogabile via `STATUS <orchestration_id>` con progress (nodi completati/totali/falliti).
- [ ] **16.4** Raccolta risultati: output di ogni sub-task accessibile dall'orchestratore come contesto per i nodi successivi.
- [ ] **16.5** Policy fallimento configurabile: `fail_fast` (abort tutto al primo errore) vs `best_effort` (continua nodi indipendenti).
- [ ] **16.6** Test unitari: grafo lineare (A→B→C), grafo parallelo (A→{B,C}→D), grafo con nodo fallito + policy.
- [ ] **16.7** Suite test verde, clippy pulito.

**Note di design**
- L'orchestratore non è un processo LLM speciale: è logica kernel-side che gestisce lo scheduling dei sub-task usando il `ProcessScheduler` esistente.
- I risultati intermedi passano tramite NeuralMemory (già persistente) oppure buffer in-kernel leggero.
- Il formato task graph è JSON per interoperabilità con la GUI e client esterni.

**Stima:** ~4-6h

---

### 17) Benchmark comparativo swarm
**Status:** `TODO`

**Obiettivi**
- Chiudere l'ultimo DoD rimasto dalla milestone 9 (benchmark swarm vs single-model).
- Produrre dati quantitativi su latenza, throughput e qualità con routing multi-modello vs modello singolo.

**DoD**
- [ ] Script benchmark riproducibile (`src/eval_swarm.py` o Rust integration test).
- [ ] Report JSON in `reports/` con metriche: latency p50/p95, tokens/sec, task completion rate.
- [ ] Almeno 2 scenari: (a) tutti i task su un solo modello, (b) routing capability-aware su 2+ modelli.
- [ ] Analisi regressione documentata nel report.

**Prerequisiti:** Almeno 2 modelli `.gguf` disponibili in `models/`.

**Stima:** ~2-3h

---

## Roadmap futura — Fase 3: Intelligenza agentica

### 18) Tool registry dinamico
**Status:** `TODO`

**Obiettivi**
- Sostituire il registry hardcoded di tool (`python`, `write_file`, `calc`) con un sistema dinamico.
- Permettere registrazione/discovery di tool a runtime da parte di agenti o plugin esterni.
- Abilitare estensibilità senza modificare il codice kernel.

**DoD**
- [ ] **18.1** `ToolDescriptor` struct: nome, descrizione, schema input (JSON Schema), backend (host/wasm/remote).
- [ ] **18.2** `ToolRegistry` con `register(descriptor)` / `unregister(name)` / `list()` / `resolve(name)`.
- [ ] **18.3** Opcodes protocollo: `REGISTER_TOOL <json>`, `LIST_TOOLS`, `TOOL_INFO <name>`.
- [ ] **18.4** Dispatch syscall aggiornato: lookup nel registry prima del dispatch hardcoded.
- [ ] **18.5** Tool remoto: possibilità di registrare un tool che fa HTTP call a un endpoint esterno.
- [ ] **18.6** Integrazione GUI: pannello tool con lista, dettagli, register/unregister.
- [ ] **18.7** Test: register + invoke, unregister + fallback, tool remoto mock.
- [ ] **18.8** Suite test verde, clippy pulito.

**Note di design**
- Il registry è in-memory (non persistente nel primo rilascio). Checkpoint/restore (M14) può serializzarlo in futuro.
- I tool WASM sono già supportati come backend sandbox — il registry li rende discoverable programmaticamente.
- Il JSON Schema dei tool è compatibile con il formato OpenAI function calling per interoperabilità.

**Stima:** ~4-6h

---

### 19) Context window management
**Status:** `TODO`

**Obiettivi**
- Gestire intelligentemente la finestra di contesto LLM per processi long-running.
- Evitare troncamento silenzioso su prompt lunghi e conversazioni multi-turn.
- Abilitare strategie di compressione contesto (summarization, sliding window, retrieval).

**DoD**
- [ ] **19.1** Tracking context usage: ogni processo traccia token count corrente vs window size del modello caricato.
- [ ] **19.2** Strategia `sliding_window`: mantiene ultimi N token, scarta i più vecchi con boundary allineato a turni conversazione.
- [ ] **19.3** Strategia `summarize`: quando il contesto supera soglia, genera un riassunto dei turni più vecchi usando il modello stesso e lo sostituisce al contesto originale.
- [ ] **19.4** Strategia `retrieve`: integrazione con NeuralMemory per storage/retrieval semantico di frammenti di contesto evicted (RAG-like from memory store).
- [ ] **19.5** Strategia selezionabile per processo via hint su `EXEC` (`context_strategy=sliding|summarize|retrieve`).
- [ ] **19.6** Metriche context in `STATUS`: `context_tokens_used`, `context_window_size`, `context_compressions`.
- [ ] **19.7** Test unitari: overflow detection, sliding window boundary, summarize trigger.
- [ ] **19.8** Suite test verde, clippy pulito.

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
  └─ M17  Benchmark swarm            ~2-3h    (validazione)

Fase 3 — Intelligenza agentica (aprile 2026)
  ├─ M18  Tool registry dinamico     ~4-6h    (estensibilità)
  └─ M19  Context window mgmt        ~6-10h   (qualità agentica)
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
