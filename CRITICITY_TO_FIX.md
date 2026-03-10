# AgenticOS — Criticità da Risolvere

Fonte: analisi completa del codebase del 7 marzo 2026.
Questo file e' il piano operativo per chiudere tutte le criticita' prima delle prossime milestone.

Decisione architetturale del 2026-03-07:
- AgenticOS prosegue come **AI workstation OS local-first, single-node**.
- Nel breve termine il focus e' correttezza, coerenza protocollo/GUI, affidabilita' operativa e disciplina del contesto.
- `mio` resta lo stack I/O di riferimento; Tokio verra' rivalutato solo se i requisiti usciranno davvero dal perimetro local-first.

---

## Indice criticità

| # | Severità | Titolo | Moduli coinvolti | Stima |
|---|----------|--------|------------------|-------|
| C1 | ✅ DONE | Inferenza bloccante nell'event loop | `runtime.rs`, `main.rs`, `engine/`, `inference_worker.rs` | ~8-12h |
| C2 | ✅ DONE | Incoerenza modello di concorrenza (`Rc` vs `Arc`) | `main.rs`, `runtime.rs`, `memory/` | ~4-6h |
| C3 | ✅ DONE | Nessuna autenticazione sul protocollo TCP | `transport/`, `protocol.rs`, `commands/` | ~3-4h |
| C4 | ✅ DONE | `execute_command` monolitico (720 righe, 11 parametri) | `commands/mod.rs` | ~3-4h |
| C5 | ✅ DONE | STATUS flat non strutturato → parsing regex fragile | `commands/mod.rs`, GUI `widgets/` | ~4-6h |
| C6 | ✅ DONE | Metriche globali statiche (`OnceLock<Mutex>`) | `commands/metrics.rs`, `tools.rs` | ~2-3h |
| C7 | ✅ DONE | Qwen model reload completo per ogni spawn | `backend.rs`, `engine/lifecycle.rs` | ~2-3h |
| C8 | ✅ DONE | GUI: connessione TCP nuova per ogni richiesta | `gui/protocol_client.py` | ~2-3h |
| C9 | ✅ DONE | GUI: thread spawn per ogni richiesta (no pool) | `gui/app.py` | ~1-2h |
| C10 | ✅ DONE | GUI: HTML rebuild completo ad ogni token di streaming | `gui/widgets/chat.py` | ~2-3h |
| C11 | ✅ DONE | GUI: N+1 query problem nella sezione Processes | `gui/widgets/processes.py`, `gui/app.py` | ~1-2h |
| C12 | ✅ DONE | Porta 6379 hardcoded (conflitto Redis) | `main.rs`, `gui/protocol_client.py` | ~1h |
| C13 | ✅ DONE | Nessun recovery per swap worker thread crash | `memory/swap.rs` | ~1-2h |
| C14 | ✅ DONE | Debito tecnico minore (dead_code, lingua mista, unwrap) | Vari | ~2h |
| C15 | ✅ DONE | Contratto `MEMW` incoerente tra backend e GUI | `commands/parsing.rs`, `memory/core.rs`, `gui/widgets/memory.py` | ~2-4h |
| C16 | ✅ DONE | `RESTORE` parziale e non transazionale | `checkpoint.rs`, `commands/checkpoint_cmd.rs`, `commands/status.rs` | ~3-5h |
| C17 | ✅ DONE | Hint workload Chat non inoltrato al kernel | `gui/app.py`, `gui/widgets/chat.py` | ~0.5-1h |
| C18 | ✅ DONE | API modelli ancora flat-text invece di JSON | `model_catalog.rs`, `commands/model.rs`, `gui/widgets/models.py` | ~2-3h |
| C19 | ✅ DONE | Metriche Chat approssimate, non affidabili | `gui/widgets/chat.py`, `gui/app.py`, `commands/status.rs` | ~2-4h |
| C20 | ✅ DONE | Crescita non limitata del contesto in orchestrazione | `orchestrator.rs`, `commands/status.rs`, `runtime.rs` | ~4-6h |
| C21 | ✅ DONE | Drift di documentazione e semantica runtime | `ARCHITECTURE.md`, `ROADMAP.md`, `gui/README.md` | ~1-2h |
| C22 | ✅ DONE | Backend runtime hardcoded per famiglia/modello | `backend.rs`, `engine/lifecycle.rs`, `process.rs` | ~5-8h |
| C23 | ✅ DONE | Prompt templates, stop markers e special tokens hardcoded | `prompting.rs`, `engine/tokenizer.rs` | ~4-7h |
| C24 | ✅ DONE | Metadata modello non first-class | `model_catalog.rs`, `commands/model.rs`, GUI `widgets/models.py` | ~5-8h |
| C25 | ✅ DONE | Routing troppo statico e family-based | `model_catalog.rs`, GUI `widgets/models.py` | ~4-6h |
| C26 | ✅ DONE | Assenza di driver plane esterno / model-driver resolution | `backend.rs`, `engine/`, `model_catalog.rs` | ~6-10h |
| C27 | ✅ DONE | Stato runtime troppo accoppiato a `PromptFamily` globale | `main.rs`, `runtime.rs`, `commands/`, `engine/` | ~4-8h |

**Effort storico (C1-C14): ~35-50h**

**Effort residuo local-first hardening (C15-C21): 0h — chiuso il 2026-03-07**

**Effort stimato future-model-flexibility residuo: focus spostato su pilot `Qwen3.5` e integrazioni future**

---

## C1 — Inferenza bloccante nell'event loop — ✅ DONE

### Soluzione implementata: checkout/checkin pattern

Invece di spostare l'intero `LLMEngine` in un thread dedicato (che richiederebbe una riscrittura profonda), si è adottato il pattern **checkout/checkin**:

1. `run_engine_tick()` fa `remove()` del `AgentProcess` dalla HashMap dell'engine ("checkout")
2. Lo invia al worker thread via `mpsc::Sender<InferenceCmd>`
3. Il worker esegue la forward pass (prefill + sample) sulla `AgentProcess` che possiede
4. Il worker restituisce `(AgentProcess, token_id, finished)` via `mpsc::Sender<InferenceResult>`
5. Il main thread fa `try_recv()`, re-inserisce il processo nell'engine ("checkin"), decodifica il token e gestisce syscall/delivery

**Vantaggi:**
- L'event loop non è più bloccato durante l'inferenza
- PING/STATUS rispondono immediatamente anche sotto load
- `AgentProcess` è `Send` (verificato con compile-time assert)
- 94/94 test passano senza modifiche semantiche
- Zero dipendenze nuove (solo `std::sync::mpsc` e `std::thread`)

**File coinvolti:**
- `src/inference_worker.rs` — nuovo modulo con `InferenceCmd`, `InferenceResult`, `spawn_worker()`, `run_step()`
- `src/runtime.rs` — riscrittura di `run_engine_tick()`: drain results → decode → syscall scan → client delivery → checkout & send → orchestrator advance
- `src/main.rs` — `Kernel` struct con `cmd_tx`, `result_rx`, `in_flight: HashSet<u64>`, `pending_kills: Vec<u64>`, `worker_handle`
- `src/commands/context.rs` — `CommandContext` con `in_flight`, `pending_kills`
- `src/commands/process_cmd.rs` — KILL/TERM su PID in-flight → deferred via `pending_kills`
- `src/commands/model.rs` — LOAD rifiutato se ci sono processi in-flight
- `src/commands/status.rs` — STATUS mostra `in_flight_processes` e `in_flight_pids`

**Nota architetturale — Tokio migration:**
Il pattern checkout/checkin è una scelta consapevole rispetto alla migrazione completa a Tokio. Tokio richiederebbe riscrivere ~60-70% del codebase Rust (mio event loop → tokio runtime, `Rc<RefCell<NeuralMemory>>` → `Arc<Mutex<>>`, tutti i test). Il trigger per rivalutare Tokio è: "3+ worker thread indipendenti" o requisiti di kernel distribuito. Vedi ROADMAP.md per dettagli.

### DoD

- [x] Inferenza avviene in thread dedicato
- [x] Event loop risponde a PING in <10ms sotto load
- [x] Tutti i 94 test esistenti passano
- [x] Clippy pulito (solo warning pre-esistenti)

---

## C2 — Incoerenza modello di concorrenza — ✅ DONE

### Problema (risolto)

`NeuralMemory` era wrappato in `Rc<RefCell<>>` (single-thread only), mentre `LLMEngine` era in `Arc<Mutex<>>` (thread-safe). Incoerenza rimossa.

### Soluzione implementata

Modello di concorrenza unificato: **tutto lo stato è owned dal `Kernel` struct** sul main thread, passato via `&mut` split borrows (NLL). Nessun wrapper di interior mutability.

- **C2.1** ✅ `Arc<Mutex<Option<LLMEngine>>>` → `Option<LLMEngine>` come campo di `Kernel`
- **C2.2/C2.3** ✅ Decisione: NeuralMemory resta esclusiva del main thread. Rimosso `Rc<RefCell>` in favore di ownership diretta.
- **C2.4** ✅ `NeuralMemory` passata per `&mut` ovunque. Zero overhead da borrow checking a runtime.

File modificati: `main.rs`, `runtime.rs`, `transport/io.rs`, `commands/context.rs`, `commands/mod.rs`, tutti i command handler (`model.rs`, `exec.rs`, `process_cmd.rs`, `status.rs`, `misc.rs`, `memory_cmd.rs`, `orchestration_cmd.rs`, `checkpoint_cmd.rs`), `transport/mod.rs` (test).

#### DoD

- [x] Un solo pattern di concurrency per ogni componente, documentato
- [x] Nessun `Arc<Mutex>` su componenti single-thread-only
- [x] Nessun `Rc<RefCell>` su componenti multi-thread
- [x] Compilazione e test verdi (94/94)

---

## C3 — Nessuna autenticazione TCP

### Problema

Il kernel ascolta su `127.0.0.1:6379` senza alcuna forma di autenticazione. Qualsiasi processo locale può:
- Eseguire `SHUTDOWN`
- Eseguire codice Python arbitrario via EXEC → syscall PYTHON
- Leggere/scrivere file nel workspace

Anche su localhost, su un sistema multi-utente o con malware locale, questo è un rischio. Inoltre la porta 6379 è quella standard di Redis.

### Piano di fix

**Approccio: token di autenticazione semplice, tipo Redis AUTH.**

#### Sub-task

1. **C3.1** Generare un token random al boot del kernel (32 byte, hex-encoded), scriverlo in `workspace/.kernel_token`. La GUI lo legge da lì.
2. **C3.2** Nuovo opcode `AUTH <token>` che deve essere il primo comando su ogni connessione. Se manca o è errato → close connection.
3. **C3.3** Stato `unauthenticated` nel `Client` struct. Se il client non è autenticato, rifiutare qualsiasi comando tranne `AUTH` con `-ERR AUTH_REQUIRED`.
4. **C3.4** Flag env `AGENTIC_AUTH_DISABLED=true` per disabilitare auth in sviluppo/test.
5. **C3.5** Aggiornare `ProtocolClient` Python per inviare `AUTH` automaticamente alla connessione.
6. **C3.6** Aggiornare la test suite TCP (13 test) per gestire auth.

#### DoD

- [x] Auth obbligatorio per default
- [x] Token generato al boot, salvato in file
- [x] GUI legge e usa il token automaticamente
- [x] Bypass con env var per test
- [x] Suite test verde (96/96)

---

## C4 — `execute_command` monolitico

### Problema

`execute_command` in `commands/mod.rs` è un singolo match con 20 bracci (~720 righe), che prende 11 parametri. Viola il principio di single responsibility, rende difficile la navigazione e il testing granulare.

### Piano di fix

#### Sub-task

1. **C4.1** Definire `CommandContext` struct:
   ```rust
   pub struct CommandContext<'a> {
       pub client: &'a mut Client,
       pub memory: &'a Rc<RefCell<NeuralMemory>>,
       pub engine_state: &'a Arc<Mutex<Option<LLMEngine>>>,
       pub model_catalog: &'a mut ModelCatalog,
       pub active_family: &'a mut PromptFamily,
       pub scheduler: &'a mut ProcessScheduler,
       pub orchestrator: &'a mut Orchestrator,
       pub client_id: usize,
       pub shutdown_requested: &'a Arc<AtomicBool>,
   }
   ```
2. **C4.2** Estrarre ogni braccio del match in una funzione dedicata in file separati:
   - `commands/model.rs` → `handle_load`, `handle_list_models`, `handle_select_model`, `handle_model_info`
   - `commands/exec.rs` → `handle_exec`
   - `commands/process.rs` → `handle_term`, `handle_kill`, `handle_status`
   - `commands/scheduler_cmd.rs` → `handle_set_priority`, `handle_get_quota`, `handle_set_quota`
   - `commands/memory_cmd.rs` → `handle_memw`, `handle_checkpoint`, `handle_restore`
   - `commands/orchestration_cmd.rs` → `handle_orchestrate`
   - `commands/misc.rs` → `handle_ping`, `handle_shutdown`, `handle_set_gen`, `handle_get_gen`
3. **C4.3** `execute_command` diventa un dispatch di ~40 righe che crea il contesto e chiama l'handler.
4. **C4.4** Ogni handler restituisce `Vec<u8>` (la response), il dispatch si occupa di `record_command()` e `client.output_buffer.extend()`.

#### DoD

- [x] `execute_command` < 60 righe
- [x] Ogni handler in un file dedicato
- [x] `CommandContext` elimina i 11 parametri
- [x] Suite test invariata e verde

---

## C5 — STATUS flat → JSON strutturato

### Problema

Il STATUS emette una stringa enorme:
```
uptime_s=123 total_commands=42 ... mem_active=true mem_total_blocks=256 ...
```

La GUI parsa con `re.search(r"key=([^ ]+)")` in 5+ widget diversi. Fragile, duplicato, e non estensibile.

### Piano di fix

#### Sub-task

1. **C5.1** Definire `StatusResponse` struct serializzabile con serde:
   ```rust
   #[derive(Serialize)]
   pub struct StatusResponse {
       pub uptime_secs: u64,
       pub total_commands: u64,
       pub active_processes: Vec<ProcessStatusEntry>,
       pub memory: MemoryStatusEntry,
       pub scheduler: SchedulerStatusEntry,
       pub model: ModelStatusEntry,
       // ...
   }
   ```
2. **C5.2** Lo handler STATUS serializza in JSON con `serde_json::to_string()`.
3. **C5.3** Il formato wire resta `+OK STATUS <len>\r\n<json>`. Nessun cambio al framing.
4. **C5.4** Aggiornare la GUI: `json.loads(payload)` sostituisce tutte le regex. Creare `parse_status()` helper unificato.
5. **C5.5** Includere dati per-PID nel STATUS globale (risolve anche C11).
6. **C5.6** Retrocompatibilità: il vecchio formato può essere mantenuto con `STATUS --legacy` o deprecato direttamente (la GUI è l'unico client).

#### DoD

- [x] STATUS restituisce JSON valido
- [x] GUI parsa con `json.loads`, senza regex
- [x] Widget ricevono dict Python (nessuna regex `_ex`)
- [x] Anche GET_QUOTA e ORCHESTRATE restituiscono JSON
- [x] Nessuna regressione funzionale (96/96 test)

---

## C6 — Metriche globali statiche — ✅ DONE

### Problema (risolto)

`metrics.rs` usava `static OnceLock<Mutex<MetricsState>>` e `tools.rs` usava `static RATE_STATES`. Singletons eliminati.

### Soluzione implementata

- **C6.1** ✅ `MetricsState` è campo di `Kernel`, con `started_at: Instant` integrato.
- **C6.2** ✅ `&mut MetricsState` passato ai command handler via `CommandContext.metrics`.
- **C6.3** ✅ `RATE_STATES` → `SyscallRateMap` come campo di `Kernel`, passato via `run_engine_tick` → `dispatch_process_syscall` → `handle_syscall`.
- **C6.4** ✅ `started_at: Instant` è campo di `MetricsState`, inizializzato in `MetricsState::new()`.

File modificati: `commands/metrics.rs`, `tools.rs`, `commands/context.rs`, `commands/mod.rs`, `runtime.rs`, `main.rs`, `transport/io.rs`, `transport/mod.rs` (test). Test tools.rs aggiornati a istanze locali.

#### DoD

- [x] Nessuna `static` mutabile per stato applicativo
- [x] Test possono istanziare metriche indipendenti
- [x] Compilazione e test verdi (94/94)

---

## C7 — Qwen model reload per ogni spawn

### Problema

`RuntimeModel::duplicate_if_supported()` restituisce `None` per Qwen2, causando un reload completo dei pesi da disco (~9GB) ad ogni `spawn_process()`. Questo rende la concorrenza multi-processo con Qwen impraticabile.

### Piano di fix

1. **C7.1** Investigare se `candle_transformers::models::quantized_qwen2::ModelWeights` implementa `Clone`. Se sì, abilitarlo come per Llama.
2. **C7.2** Se `Clone` non è disponibile, investigare `Arc`-wrapping dei layer condivisi (i pesi sono read-only durante inference).
3. **C7.3** Se nessun approccio è viable con candle 0.9, documentare il limite e implementare un guard: max 1 processo per Qwen contemporaneamente, con errore chiaro se si tenta di spawnarne un secondo.
4. **C7.4** Aggiornare `ProcessScheduler` per enforcare il limit se necessario.

#### DoD

- [x] Guard in `spawn_process()`: se backend non clonabile e processi già attivi → errore chiaro
- [x] Documentato in `backend.rs` (`duplicate_if_supported` doc comment)
- [x] Test coverage indiretto (errore path hit when duplicate returns None + processes exist)

---

## C8 — GUI: connessione TCP nuova per ogni richiesta — ✅ DONE

### Soluzione implementata

`ProtocolClient` now maintains a persistent TCP socket protected by `threading.Lock`. Control-plane requests (`send_once`) reuse the same connection, with automatic reconnect on disconnect. `exec_stream` keeps dedicated per-stream sockets (long-running streaming). A `close()` method is exposed for shutdown cleanup.

**File modificati:** `gui/protocol_client.py`

### Problema

`ProtocolClient.send_once()` apre un nuovo `socket.socket()` per ogni richiesta. Con STATUS polling ogni 5s + audit ogni 0.5s + modelli ogni 15s → ~15+ connessioni/minuto.

### Piano di fix

1. **C8.1** Aggiungere `PersistentClient` class che mantiene un socket aperto con reconnect on error.
2. **C8.2** Thread-safety: il client è usato da thread multipli → `threading.Lock` per serializzare send/recv, oppure un singolo IO thread con coda di richieste.
3. **C8.3** Approccio consigliato: un singolo "protocol IO thread" con una `queue.Queue` di richieste. I caller fanno `queue.put(request)` e attendono un `Future`/`Event` per la risposta.
4. **C8.4** `exec_stream` mantiene il pattern connessione dedicata (streaming lungo).

#### DoD

- [x] Un singolo socket per le richieste control-plane (non EXEC)
- [x] Reconnect automatico su disconnect
- [x] Thread-safe
- [x] Nessuna regressione funzionale GUI

---

## C9 — GUI: thread spawn per ogni richiesta — ✅ DONE

### Soluzione implementata

`MainWindow` now owns a `ThreadPoolExecutor(max_workers=4)`. All `_dispatch_control_request`, `_refresh_status`, and `_load_model` use `self._executor.submit(task)` instead of `threading.Thread`. EXEC streaming retains a dedicated `threading.Thread` (long-running, shouldn't block the pool). A `closeEvent` was added to stop all timers, shutdown the executor, and close the persistent TCP client.

**File modificati:** `gui/app.py`

### Problema

Ogni `_dispatch_control_request()` crea un `threading.Thread(daemon=True)`. Nessun pooling, nessun limit.

### Piano di fix

1. **C9.1** Creare un `ThreadPoolExecutor(max_workers=4)` come attributo di `MainWindow`.
2. **C9.2** Sostituire `threading.Thread(target=task).start()` con `self._executor.submit(task)`.
3. **C9.3** Cleanup in `closeEvent`: `self._executor.shutdown(wait=False)`.

**Nota**: se si implementa C8 con un IO thread dedicato, C9 diventa parzialmente risolto (le richieste control non creano più thread). Restano thread per EXEC stream e operazioni lunghe (LOAD).

#### DoD

- [x] Max 4 thread concorrenti per richieste protocol
- [x] `closeEvent` fa cleanup
- [x] Nessuna regressione

---

## C10 — GUI: HTML rebuild completo ad ogni token — ✅ DONE

### Soluzione implementata

`ChatSection` now uses a render-throttle approach: `append_assistant_chunk()` sets a `_render_dirty` flag instead of calling `_refresh_display()` directly. A `QTimer` at 200ms intervals (`_flush_render`) checks the flag and renders at most 5 times/second. Structural changes (new bubbles, stream finish) still call `_refresh_display()` immediately. No text loss.

**File modificati:** `gui/widgets/chat.py`

### Problema

`ChatSection._refresh_display()` chiama `self.chat_display.setHtml()` con l'**intero contenuto** ad ogni chunk di streaming. Con 50 bubble, questo ricrea il DOM HTML completo ~10-20 volte al secondo durante inferenza.

### Piano di fix

1. **C10.1** Usare `QTextCursor` per modificare solo l'ultimo bubble (append-only durante streaming).
2. **C10.2** Alternativa più semplice: **throttle** il rendering. Accumulare i chunk in buffer e renderizzare max ogni 200ms con un timer dedicato.
3. **C10.3** Al termine dello streaming (`finish_assistant_message`), fare un render completo finale una sola volta.

**Approccio consigliato: C10.2 (throttle) è più semplice e meno rischioso di C10.1.**

#### DoD

- [x] Nessun `setHtml()` per ogni token durante streaming
- [x] Refresh visivo ≤5 volte/secondo durante streaming
- [x] Nessuna perdita di testo

---

## C11 — GUI: N+1 query per Processes — ✅ DONE

### Soluzione implementata

The global Rust STATUS response now includes `active_processes: Vec<PidStatusResponse>` with full per-PID detail (workload, priority, tokens, quotas, elapsed, etc.) embedded in the single response. The GUI `ProcessesSection.update_from_status()` populates the table directly from this inline data. The N+1 `status_pid_requested.emit(pid)` loop has been removed entirely. The `status_pid_requested` signal is still available for on-demand detail refresh (user-initiated "STATUS <PID>" button click).

**File modificati:** `src/commands/status.rs`, `gui/widgets/processes.py`

### Problema

`ProcessesSection.update_from_status()` emette `status_pid_requested` per ogni PID attivo, generando N richieste TCP aggiuntive ad ogni refresh.

### Piano di fix

**Dipendenza: C5 (STATUS JSON) risolve alla radice** — includendo i dati per-PID nel response JSON globale.

1. **C11.1** Con C5 risolto, il STATUS globale include `active_processes: [{ pid, workload, priority, tokens, ... }]`.
2. **C11.2** Rimuovere il loop `for pid in all_pids: self.status_pid_requested.emit(pid)`.
3. **C11.3** `update_from_status()` popola direttamente la tabella dal JSON.

#### DoD

- [x] Zero richieste extra per popolare la tabella processi
- [x] Tabella completa dopo un singolo STATUS
- [x] Nessun `status_pid_requested` nel refresh periodico

---

## C12 — Porta 6379 hardcoded

### Problema

`127.0.0.1:6379` è hardcoded in `main.rs`. Conflitto con Redis. Non configurabile.

### Piano di fix

1. **C12.1** Leggere porta da env var `AGENTIC_PORT` con default `6380` (evita conflitto Redis).
2. **C12.2** Aggiornare `ProtocolClient` e `KernelProcessManager` per leggere la stessa env var o accettarla come parametro.
3. **C12.3** Aggiornare ARCHITECTURE.md con la nuova porta.

#### DoD

- [x] Porta configurabile via env var
- [x] Default 6380 (non 6379)
- [x] GUI e kernel usano la stessa porta

---

## C13 — Swap worker thread senza recovery — ✅ DONE

### Soluzione implementata

`SwapManager` now tracks a `worker_crashes` counter. When `poll_events()` detects `TryRecvError::Disconnected`:
1. Logs `tracing::error!` with the crash count
2. Marks all in-flight waiting PIDs as failed (with `SwapEvent` entries)
3. On the first crash, attempts one automatic re-spawn via `spawn_worker()`
4. On a second crash, disables swap permanently with a log message
The `worker_crashes` counter is exposed in `MemorySnapshot` → `MemoryStatus` → STATUS JSON for diagnostics.

**File modificati:** `src/memory/swap.rs`, `src/memory/types.rs`, `src/memory/core.rs`, `src/commands/status.rs`

### Problema

Il swap worker thread in `memory/swap.rs` è spawned una volta. Se panics, il channel si disconnette e `poll_events()` disabilita lo swap silenziosamente. Nessun retry, nessun allarme visibile.

### Piano di fix

1. **C13.1** In `poll_events()`, quando si rileva `TryRecvError::Disconnected`, emettere un `tracing::error!` + incrementare un contatore `swap_worker_crashes`.
2. **C13.2** Tentare un re-spawn automatico del worker (1 retry).
3. **C13.3** Il contatore è visibile in STATUS per diagnostica.

#### DoD

- [x] Log esplicito al crash del worker
- [x] Un tentativo di re-spawn
- [x] Contatore visibile via STATUS

---

## Nuove criticita' aperte — Hardening local-first (C15-C21)

## C15 — Contratto `MEMW` incoerente tra protocollo, backend e GUI — ✅ DONE

### Problema

La GUI presenta `MEMW` come input testuale libero (`pid|text`), il parser accetta quel formato, ma il backend interpreta il payload come sequenza di `f32` grezzi. Il path di scrittura usa `chunks_exact(4)` e puo' quindi scartare byte finali non allineati senza errore esplicito. Per una AI workstation OS local-first questo e' un problema di correttezza del contratto, non solo di UX.

### Piano di fix

1. **C15.1** Definire un solo contratto canonico per `MEMW`:
  - `MEMW` low-level binario: `<pid>\n<raw-bytes>`
  - il body deve essere esplicitamente validato per il formato atteso dal backend
2. **C15.2** Se il backend continua a trattare il payload come `f32`, rifiutare input non multipli di 4 con errore chiaro (`MEMW_INVALID_ALIGNMENT`). Nessun troncamento silenzioso.
3. **C15.3** Aggiornare la GUI Memory:
  - rimuovere il messaggio ambiguo `Raw bytes / text`
  - presentare `MEMW` come strumento diagnostico low-level, non come editor testuale generico
4. **C15.4** Aggiungere test di regressione: payload non allineato, payload binario valido, mismatch parser/GUI.

**Esito 2026-03-07**
- Contratto canonico unificato su `<pid>\n<raw-bytes>`.
- Sintassi pipe rimossa dal parser; payload non allineati a 4 byte rifiutati con errore esplicito.
- La GUI Memory presenta `MEMW` come tool diagnostico low-level coerente con il backend.
- Test di regressione aggiunti per parser canonico e payload disallineato.

#### DoD

- [x] `MEMW` ha una semantica unica e documentata
- [x] Nessun byte viene perso o ignorato silenziosamente
- [x] GUI e backend usano lo stesso contratto
- [x] Test di regressione coprono gli input invalidi

---

## C16 — `RESTORE` parziale e non transazionale — ✅ DONE

### Problema

L'opcode `RESTORE` oggi ricarica solo parte del metadata e non sostituisce in modo atomico lo stato restore-able del kernel. Il nome suggerisce un ripristino forte, ma l'implementazione attuale si comporta piu' come un merge parziale di scheduler state + selected model. Per un sistema local-first questo crea aspettative sbagliate e possibili inconsistenze operative.

### Piano di fix

1. **C16.1** Scegliere una semantica esplicita:
  - oppure `RESTORE` diventa un restore transazionale della porzione restore-able
  - oppure l'opcode e la UI vengono rinominati in modo piu' onesto (`RESTORE_METADATA` / `APPLY_SNAPSHOT`)
2. **C16.2** Se si mantiene `RESTORE`, pulire lo stato restore-able esistente prima di applicare lo snapshot (scheduler entries, selected model restore-able, eventuale stato derivato).
3. **C16.3** Esporre chiaramente in risposta e in `STATUS` cosa NON viene ripristinato: processi live, model weights, tensor data, output buffers.
4. **C16.4** Aggiornare GUI e documentazione per evitare l'aspettativa di resume completo dell'inference.
5. **C16.5** Aggiungere test per: restore su stato gia' popolato, restore idempotente, clear+apply, warning espliciti su processi orfani.

**Esito 2026-03-07**
- `RESTORE` e' stato reso `metadata_only_clear_and_apply`: clear dello scheduler restore-able, reset dello stato derivato, applicazione esplicita dello snapshot.
- Il comando rifiuta il restore su kernel non idle per evitare merge impliciti con processi vivi.
- La risposta protocollo e la GUI espongono i limiti del restore in formato machine-readable.
- Test di regressione aggiunti sul clear+apply dello scheduler.

#### DoD

- [x] La semantica di `RESTORE` e' esplicita e verificabile
- [x] Nessun merge implicito non documentato
- [x] Risposta protocollo e GUI comunicano i limiti del restore
- [x] Test coprono i casi di restore su stato preesistente

---

## C17 — Hint workload Chat non inoltrato al kernel — ✅ DONE

### Problema

La sezione Chat espone il dropdown `auto/fast/code/reasoning/general`, ma il valore non arriva realmente al backend nel path `EXEC`. Questo riduce il valore operativo della GUI e crea una affordance ingannevole.

### Piano di fix

1. **C17.1** Far serializzare il workload hint nel prompt/protocollo in modo esplicito e compatibile con `parse_workload_hint()`.
2. **C17.2** Mantenere `auto` come comportamento di default senza hint aggiuntivo.
3. **C17.3** Aggiungere test GUI-side o smoke test che verifichino il forwarding del hint.

**Esito 2026-03-07**
- La GUI prepone `capability=<hint>;` al prompt solo quando il selettore non e' `auto`.
- `auto` preserva il comportamento corrente senza hint aggiuntivo.
- La critica sull'affordance ingannevole e' stata recepita: la UI non promette piu' un routing che il kernel non vede.

#### DoD

- [x] Se l'utente seleziona `code`, il kernel riceve davvero il relativo hint
- [x] `auto` non altera il comportamento corrente
- [x] Nessun mismatch tra UI mostrata e comportamento effettivo

---

## C18 — API modelli ancora flat-text invece di JSON — ✅ DONE

### Problema

`STATUS`, `GET_QUOTA` e `ORCHESTRATE` sono gia' JSON, ma `LIST_MODELS` resta testo libero e la GUI lo riparsa con regex. Questo introduce una zona legacy fragile proprio in una sezione centrale per la workstation.

### Piano di fix

1. **C18.1** Restituire `LIST_MODELS` in JSON strutturato (`models: [...]`, `selected_model_id`, capability metadata, tokenizer presence).
2. **C18.2** Allineare `MODEL_INFO` allo stesso approccio, evitando stringhe multilinea non machine-readable.
3. **C18.3** Rimuovere il parsing regex da `gui/widgets/models.py`.
4. **C18.4** Facoltativo ma consigliato: esporre nel payload la routing recommendation calcolata dal backend, non solo dalla GUI.

**Esito 2026-03-07**
- `LIST_MODELS` e `MODEL_INFO` restituiscono JSON strutturato con metadata, tokenizer e routing recommendations.
- La GUI Models non usa piu' parsing regex per popolare cards e dettagli.
- Anche `eval_swarm.py` e' stato allineato al nuovo payload JSON.

#### DoD

- [x] Le API modello sono machine-readable e coerenti con il resto del protocollo
- [x] La GUI non usa regex per popolare le model cards
- [x] `LIST_MODELS` e `MODEL_INFO` condividono uno schema stabile

---

## C19 — Metriche Chat approssimate, non affidabili — ✅ DONE

### Problema

La Chat mostra token count e throughput, ma oggi li stima tramite byte ricevuti. Questo va bene per diagnostica veloce, ma non per metriche operative affidabili in una control center GUI.

### Piano di fix

1. **C19.1** Decidere la fonte canonica delle metriche: kernel-side, non GUI-side.
2. **C19.2** Esportare i contatori reali per PID gia' disponibili nello scheduler/runtime o arricchire gli eventi di fine processo.
3. **C19.3** La GUI usa metriche reali; in fallback mostra esplicitamente `approx`.
4. **C19.4** Verificare coerenza tra `STATUS`, chat bubble finale e pannello Processes.

**Esito 2026-03-07**
- Il runtime emette il marker finale con `tokens_generated` ed `elapsed_secs` reali dal scheduler.
- La Chat mostra metriche `approx` durante lo streaming e passa ai contatori reali appena disponibili.
- La formula implicita `bytes -> tok` resta solo come fallback esplicito, non come metrica opaca.

#### DoD

- [x] Token e throughput mostrati in Chat sono reali oppure etichettati chiaramente come stime
- [x] Nessuna formula implicita `bytes -> tok` nascosta alla GUI
- [x] Coerenza tra Chat e Processes per lo stesso PID

---

## C20 — Crescita non limitata del contesto in orchestrazione — ✅ DONE

### Problema

L'orchestratore accumula output completi dei task e li reinietta integralmente nei nodi dipendenti. Su workflow lunghi o output voluminosi questo puo' gonfiare prompt, latenza e memoria, degradando il caso d'uso local-first proprio dove dovrebbe essere piu' stabile.

### Piano di fix

1. **C20.1** Introdurre un cap configurabile sulla quantita' di output per task memorizzata in orchestrazione.
2. **C20.2** Truncare o compattare il contesto iniettato nei task dipendenti con marker espliciti (`[TRUNCATED]`).
3. **C20.3** Esporre in `STATUS orch:N` il numero di truncation/compression applicate.
4. **C20.4** Aggiungere test su DAG con output lungo e su grafo parallelo con aggregazione pesante.

**Esito 2026-03-07**
- Introdotto cap configurabile `AGENTIC_ORCH_MAX_OUTPUT_CHARS` per l'output memorizzato per task.
- Gli output oltre soglia vengono troncati con marker esplicito `[TRUNCATED]`.
- `STATUS orch:` e la GUI Orchestration mostrano truncation count e caratteri attualmente memorizzati.
- Delta 2026-03-10: M29 ha esteso questa disciplina con context-window management per PID e osservabilita' orchestration coerente. I task running in `STATUS orch:` espongono ora anche lo snapshot context (`context_strategy`, token usage, compression/retrieval counters), mentre la nuova strategia `retrieve` usa uno store episodico pragmatico per evitare crescita cieca del contesto live.

#### DoD

- [x] L'output orchestrato ha limiti espliciti e configurabili
- [x] Nessun grafo puo' crescere senza bound nel solo buffer di contesto
- [x] `STATUS orch:` rende visibili truncation o compaction

---

## C21 — Drift di documentazione e semantica runtime — ✅ DONE

### Problema

Parte della documentazione non riflette piu' il codice reale: autenticazione ormai presente, semantica di restore metadata-only, posizione del progetto come local-first e limiti reali dello scheduler rispetto alla concorrenza forte.

### Piano di fix

1. **C21.1** Aggiornare `ARCHITECTURE.md`, `ROADMAP.md`, `CRITICITY_TO_FIX.md` e `gui/README.md` con lo stato reale del protocollo.
2. **C21.2** Dichiarare esplicitamente che lo scheduler oggi e' di governance/ordering, non di parallelismo forte.
3. **C21.3** Dichiarare esplicitamente il focus local-first e la permanenza su `mio` nel breve termine.

**Esito 2026-03-07**
- Integrati nei documenti i punti coerenti della critica: contratti machine-readable, restore onesto, scheduler come governance locale e priorita' a operator trust.
- `ARCHITECTURE.md` e `gui/README.md` ora riflettono auth, API modelli JSON, `MEMW` canonico e restore metadata-only.
- La roadmap esplicita che il progetto resta local-first su `mio`, senza spostarsi prematuramente verso concorrenza forte.

#### DoD

- [x] Nessuna sezione documentale contraddice il comportamento reale del kernel
- [x] Focus local-first e posizione su `mio` sono espliciti
- [x] Restore metadata-only e auth sono documentati in modo coerente

---

## Nuove criticita' aperte — Future-model flexibility (C22-C27)

## C22 — Backend runtime hardcoded per famiglia/modello — ✅ DONE

### Problema

Il runtime oggi dipende ancora da backend concreti scelti nel codice, con coupling storico su `RuntimeModel`, `LLMEngine::load()` e `AgentProcess`. Questo rende costoso integrare famiglie o driver futuri e spinge ogni nuovo modello a toccare il kernel invece del solo driver.

### Piano di fix

1. **C22.1** Introdurre un contratto backend stabile (`trait` / wrapper) che separi control plane e logica di inferenza specifica.
2. **C22.2** Adattare i backend interni attuali (Llama, Qwen2) dietro il contratto senza regressioni.
3. **C22.3** Esporre identity e vincoli runtime del backend come metadata interrogabili.
4. **C22.4** Ridurre progressivamente i punti del kernel che assumono backend concreti.

**Esito 2026-03-08**
- `RuntimeModel` non e' piu' un enum chiuso: introdotto contratto `ModelBackend` e wrapper trait-based in `backend.rs`.
- I backend interni `Llama` e `Qwen2` sono gia' adattati dietro il nuovo contratto.
- `engine/lifecycle.rs` usa gia' `backend_id()` e `family()` del backend risolto.
- Catalogo e info modello espongono il backend risolto e i vincoli di caricamento del driver (`driver_available`, `driver_load_supported`).
- `model_catalog.rs` centralizza ora il load contract in `ResolvedModelTarget`: path, family, tokenizer hint, metadata e driver resolution vengono risolti una sola volta nel catalogo.
- `commands/model.rs`, `commands/exec.rs` e `engine/lifecycle.rs` consumano il target risolto invece di ricostruire manualmente family/backend/metadata nei path critici del kernel.
- Validazione: `cargo test --release` verde a 112/112; nessuna regressione nei test del catalogo, del transport o del runtime.

#### DoD

- [x] Contratto backend introdotto
- [x] Backend interni adattati dietro il contratto
- [x] Nessun punto critico del kernel dipende ancora da enum/dispatch concreti
- [x] Metadata backend esposti in modo stabile a protocollo/catalogo

---

## C23 — Prompt templates, stop markers e special tokens hardcoded — ✅ DONE

### Problema

`prompting.rs` e `engine/tokenizer.rs` dipendono da assunzioni hardcoded per famiglia. Questo blocca l'integrazione fluida di modelli che richiedono template o token speciali diversi, anche se il driver di inferenza fosse gia' pronto.

### Piano di fix

1. **C23.1** Definire un path metadata-driven per chat template, stop markers e token names.
2. **C23.2** Consumare tali metadata nel prompting runtime.
3. **C23.3** Consumare tali metadata nella validazione tokenizer/special tokens.
4. **C23.4** Mantenere fallback hardcoded esplicito per modelli legacy privi di metadata.

**Esito 2026-03-07**
- `prompting.rs` ora supporta `chat_template`, `assistant_preamble` e `stop_markers` da metadata runtime.
- `engine/tokenizer.rs` consuma `special_tokens` da metadata quando disponibili.
- Il runtime usa helper metadata-aware per system injection, interprocess formatting e stop detection.
- Restano fallback family-based espliciti e coperti da test per i modelli legacy senza metadata.

#### DoD

- [x] Chat template runtime supportato
- [x] Special token resolution metadata-driven disponibile
- [x] Fallback legacy stabile e testato

---

## C24 — Metadata modello non first-class — ✅ DONE

### Problema

Il catalogo oggi conosce soprattutto path, famiglia e tokenizer. Mancano metadata runtime essenziali come source dei metadata, backend preference, chat template, special tokens e capability dichiarate.

### Piano di fix

1. **C24.1** Introdurre `ModelMetadata` nel catalogo come struttura esplicita.
2. **C24.2** Supportare una prima fonte pragmatica di metadata sidecar per modello.
3. **C24.3** Esporre i metadata nelle API `LIST_MODELS` e `MODEL_INFO`.
4. **C24.4** Preparare il passo successivo verso parsing nativo da GGUF/tokenizer config.

**Esito 2026-03-07**
- `ModelMetadata` introdotto in `model_catalog.rs`.
- Sidecar `metadata.json` e `<model>.metadata.json` supportati come fonte runtime iniziale.
- `LIST_MODELS` e `MODEL_INFO` arricchiti con `metadata_source`, `backend_preference`, `capabilities`, `chat_template`, `assistant_preamble`, `special_tokens` e `stop_markers`.
- La GUI Models espone metadata source, backend preference e capability dichiarate senza dipendere da assunzioni fisse per famiglia.
- Le API modello espongono anche `resolved_backend` e lo stato del driver associato alla risoluzione corrente.
- Il catalogo legge ora nativamente `general.architecture` e `tokenizer.chat_template` dal GGUF quando presenti, e ricava token speciali / stop markers dal `tokenizer.json` anche in assenza di sidecar.
- I sidecar restano supportati come overlay esplicito sopra i metadata nativi, invece di essere l'unica fonte di verita'.
- Validazione: `cargo test --release` verde a 112/112, con test mirati su parsing GGUF/tokenizer e merge dei metadata.

#### DoD

- [x] Metadata first-class nel catalogo
- [x] Sidecar metadata supportato
- [x] API modello espongono metadata base
- [x] Parsing nativo GGUF/tokenizer config pianificato e testato

---

## C25 — Routing troppo statico e family-based — ✅ DONE

### Problema

Il routing oggi privilegia famiglie via precedenze statiche. Questo e' fragile nel momento in cui nuovi modelli della stessa famiglia o backend diversi dichiarano capability operative piu' adatte.

### Piano di fix

1. **C25.1** Far dipendere `select_for_workload()` da capability dichiarate quando presenti.
2. **C25.2** Mantenere fallback family-based solo per modelli legacy o metadata incompleti.
3. **C25.3** Esporre in GUI/protocollo il razionale del routing.

**Esito parziale 2026-03-07**
- `select_for_workload()` usa gia' capability dichiarate dai metadata sidecar come prima fonte di ranking.
- Le euristiche statiche di famiglia restano fallback esplicito.
- `routing_recommendations` espone `source`, `rationale`, `capability_key` e `capability_score`.
- La GUI Models mostra il razionale del routing invece di inferire preferenze da sole famiglie `Llama/Qwen`.

#### DoD

- [x] Capability dichiarate precedono le euristiche family-based
- [x] Fallback legacy esplicito e osservabile
- [x] GUI aggiornata per mostrare capability e backend preference

---

## C26 — Assenza di driver plane esterno / model-driver resolution — ✅ DONE

### Problema

Il kernel non ha ancora una separazione netta tra control plane e driver plane. Anche dopo il trait backend, manca ancora il registry/policy che permetta di risolvere un modello verso un driver interno o esterno in modo esplicito.

### Piano di fix

1. **C26.1** Definire registry dei driver e capability minime dei backend.
2. **C26.2** Definire la policy di risoluzione modello -> driver.
3. **C26.3** Validare il disaccoppiamento con almeno un driver esterno mock/stub.
4. **C26.4** Esporre errori espliciti quando nessun driver soddisfa i requisiti del modello.

**Esito 2026-03-07**
- `backend.rs` introduce un driver registry esplicito per backend interni e stub esterni.
- Il caricamento runtime passa ora da una risoluzione modello -> driver, invece di scegliere direttamente dal solo `PromptFamily`.
- `external-llamacpp` e' registrato come stub esterno non loadable, utile per validare il piano di integrazione futura senza sporcare il runtime attuale.
- Se un modello non ha alcun driver compatibile loadable, il sistema espone un errore esplicito e machine-readable invece di fallire in modo implicito.

**Aggiornamento 2026-03-08 — Pilot Qwen3.5**
- La risoluzione driver usa ora anche `general.architecture` del GGUF, non solo la `family` logica del modello.
- `qwen35` viene scoperto e descritto dal catalogo, ma non viene piu' instradato in modo errato verso `candle.quantized_qwen2`.
- `LIST_MODELS` e `MODEL_INFO` espongono `architecture`, `resolved_backend`, `driver_resolution_source` e `driver_resolution_rationale`, rendendo esplicito quando manca un driver compatibile.
- Validazione: `cargo test` = 114 passed, 1 ignored; smoke test locale su `models/qwen3.5-9b` verde per discovery/tokenizer/architecture-aware rejection.

#### DoD

- [x] Driver registry definito
- [x] Model-driver resolution policy definita
- [x] Driver esterno mock/stub integrato nei test
- [x] Error handling esplicito per backend non risolvibili

---

## C27 — Stato runtime troppo accoppiato a `PromptFamily` globale — DONE

### Problema

`main.rs`, `runtime.rs` e parte dell'engine continuano a propagare assunzioni legate a un singleton `active_family`. Questo ostacola un vero runtime model-agnostic e rende i metadata backend/model-specific meno efficaci di quanto dovrebbero.

### Piano di fix

1. **C27.1** Ridurre il ruolo di `active_family` globale nel kernel.
2. **C27.2** Far dipendere il runtime da metadata/contract del modello o del backend corrente.
3. **C27.3** Chiudere i punti in cui syscall injection e formatting assumono ancora una famiglia globale implicita.

#### DoD

- [x] `active_family` non e' piu' il pivot implicito del runtime
- [x] Prompting/runtime leggono i metadata del modello/backend attivo
- [x] I path critici del kernel non assumono una sola family globale

**Esito 2026-03-07**
- `runtime.rs`, `commands/mod.rs`, `commands/context.rs` e `transport/io.rs` non propagano piu' `active_family` nei path di dispatch e controllo.
- `commands/exec.rs` decide i reload confrontando modello, family e driver realmente caricati/richiesti, invece di affidarsi a uno stato globale separato.
- `main.rs` e `commands/checkpoint_cmd.rs` conservano `active_family` solo come dato di snapshot/checkpoint derivato dall'engine o dal modello selezionato, non come sorgente di verita' per il runtime.
- Validazione: `cargo test --release` verde a 109/109 dopo l'aggiornamento del transport test harness al nuovo contratto.

**Aggiornamento 2026-03-08**
- Il pilot Qwen3.5 ha confermato il disaccoppiamento: il control plane rifiuta `LOAD` per architetture GGUF senza driver compatibile prima di arrivare al backend concreto, invece di far dipendere il runtime da una `PromptFamily` globale o da fallback impliciti.

---

## C14 — Debito tecnico minore

### Items

| # | Cosa | Dove | Fix |
|---|------|------|-----|
| 14.1 | `#[allow(dead_code)]` su 15+ items | `memory/core.rs`, `scheduler.rs`, `errors.rs` | Rimuovere le API non usate o togliere allow e usarle |
| 14.2 | Commenti in italiano misti a inglese | Tutto il codebase | Standardizzare su inglese |
| 14.3 | `unwrap()` su lock senza contesto | `main.rs`, `runtime.rs`, `commands/mod.rs` | Sostituire con `expect("message")` |
| 14.4 | `agent_id` sempre "1" nella GUI | `gui/protocol_client.py` | Usare un identificativo reale o rimuovere il campo dal protocollo |
| 14.5 | Temp file `agent_script_*.py` se crash | `tools.rs` | Aggiungere cleanup al boot del kernel |

#### DoD

- [x] Zero `#[allow(dead_code)]` non giustificati
- [ ] Lingua consistente nel codebase (C14.2 — deferred)
- [x] Zero `unwrap()` su lock, solo `expect()` con messaggio
- [x] Cleanup temp file al boot

---

## Ordine di esecuzione

Le criticità hanno dipendenze tra loro. L'ordine ottimale è:

```
Fase A — Fondazioni (prerequisiti per tutto il resto)
  ├─ C12  Porta configurabile              ~1h     (quick win, zero rischio)
  ├─ C14  Debito tecnico minore            ~2h     (housekeeping)
  └─ C4   Estrazione command handlers      ~3-4h   (pulisce il path per C1, C3, C5)

Fase B — Architettura core
  ├─ C1   Inferenza non-bloccante          ~8-12h  (il fix più impattante)
  ├─ C2   Modello concorrenza coerente     ~4-6h   (dipende da C1)
  └─ C6   Metriche non-statiche            ~2-3h   (dipende da C4)

Fase C — Protocollo e sicurezza
  ├─ C3   Autenticazione TCP               ~3-4h   (dipende da C4)
  ├─ C5   STATUS JSON                      ~4-6h   (dipende da C4)
  └─ C7   Qwen model sharing              ~2-3h   (indipendente)

Fase D — GUI
  ├─ C8   Connessione TCP persistente      ~2-3h   (dipende da C3)
  ├─ C9   Thread pool                      ~1-2h   (dipende da C8 parzialmente)
  ├─ C10  Chat rendering throttle          ~2-3h   (indipendente)
  ├─ C11  Eliminazione N+1 query           ~1-2h   (dipende da C5)
  └─ C13  Swap worker recovery             ~1-2h   (indipendente)

Fase E — Local-first hardening
  ├─ C15  Contratto MEMW coerente          ~2-4h   (prima di rifinire Memory GUI)
  ├─ C16  RESTORE semantica esplicita      ~3-5h   (prima di nuove feature persistence)
  ├─ C17  Workload hint Chat               ~0.5-1h (quick win UX)
  ├─ C18  API modelli JSON                 ~2-3h   (allinea Models GUI)
  ├─ C19  Metriche Chat affidabili         ~2-4h   (dipende da contatori backend)
  ├─ C20  Bound contesto orchestrazione    ~4-6h   (prima di context window mgmt)
  └─ C21  Sync documentazione              ~1-2h   (chiusura fase)

Fase F — Future-model flexibility
  ├─ C22  Backend model abstraction        ~5-8h   (fondazione architetturale)
  ├─ C23  Prompt/token metadata runtime    ~4-7h   (dipende da C24)
  ├─ C24  Metadata modello first-class     ~5-8h   (sidecar prima, native parsing dopo)
  ├─ C25  Capability routing v2            ~4-6h   (dipende da C24)
  ├─ C26  Driver plane esterno             ~6-10h  (dipende da C22)
  └─ C27  Runtime decoupling da family     ~4-8h   (chiusura trasversale)
```

### Diagramma dipendenze

```
C12 ──┐
C14 ──┼──► C4 ──┬──► C1 ──► C2
      │         ├──► C6
      │         ├──► C3 ──► C8 ──► C9
      │         └──► C5 ──► C11
      │
C7  ──────── (indipendente)
C10 ──────── (indipendente)
C13 ──────── (indipendente)

C15 ──┬──► C18
  └──► C21
C16 ─────► C21
C17 ─────► C21
C18 ─────► C21
C19 ─────► C21
C20 ─────► C21

C22 ──┬──► C24 ──► C25
  ├──► C26
  └──► C27
C23 ──┬──► C27
  └──► C25
```

---

## Regole operative

1. **Una criticità alla volta.** Completare, testare, committare prima di iniziare la successiva.
2. **Test prima del fix.** Se la criticità non ha test di regressione, scriverli prima.
3. **Nessun cambio di scope verso concorrenza forte.** I fix chiudono gap di correttezza/hardening local-first; non aprono ancora un cantiere Tokio o distributed.
4. **Controllare ROADMAP.md e CRITICITY_TO_FIX.md prima di iniziare ogni slice** per riallineare il task al piano operativo corrente.
5. **Aggiornare ROADMAP.md e CRITICITY_TO_FIX.md a fine di ogni slice** con stato, DoD, note sintetiche e validazione eseguita.
6. **Aggiornare ARCHITECTURE.md** se un fix modifica il design documentato.

---

## Template di tracking

Per ogni criticità risolta, aggiungere una entry qui sotto:

```md
### CX — Titolo
- **Status:** DONE ✅
- **Data:** YYYY-MM-DD
- **Commit:** <hash>
- **Note:** ...
```

---

## Registro completamento

### C12 — Porta configurabile
- **Status:** DONE ✅
- **Note:** Porta letta da env var `AGENTIC_PORT` con default `6380`. Aggiornati: `main.rs`, `protocol_client.py`, `app.py`, `client.py`, `eval_swarm.py`, `eval_llama3.py`, `ARCHITECTURE.md`, `gui/README.md`.

### C14 — Debito tecnico minore
- **Status:** DONE ✅ (C14.1, C14.3, C14.5 completati; C14.2 lingua mista — deferred)
- **Note:**
  - C14.1: 7 dead code items rimossi, 5 marcati `#[cfg(test)]`, 1 `#[allow]` rimosso (EngineError usato in tokenizer.rs).
  - C14.3: `.lock().unwrap()` → `.lock().expect()` in `main.rs`, `runtime.rs`, `metrics.rs`, `tools.rs`. I siti in `commands/mod.rs` risolti dalla riscrittura C4.
  - C14.5: `cleanup_stale_temp_scripts()` aggiunta a `tools.rs` e chiamata al boot in `main.rs`.

### C4 — Estrazione command handlers
- **Status:** DONE ✅
- **Note:** `commands/mod.rs` ridotto da ~745 righe a ~95 righe (slim dispatcher). Creati 10 file handler: `context.rs`, `model.rs`, `exec.rs`, `status.rs`, `process_cmd.rs`, `scheduler_cmd.rs`, `memory_cmd.rs`, `checkpoint_cmd.rs`, `orchestration_cmd.rs`, `misc.rs`. `CommandContext` struct sostituisce 11 parametri loose. 94/94 test passano.

### Decisione 2026-03-07 — Focus local-first
- **Status:** DECIDED
- **Note:** AgenticOS evolve come AI workstation OS local-first single-node. Priorita' immediata: chiudere C15-C21, consolidare protocollo/GUI/stato e rimandare la rivalutazione Tokio a quando esisteranno requisiti reali di concorrenza forte.

### C15-C21 — Hardening local-first completato
- **Status:** DONE ✅
- **Note:** Critica coerente recepita e integrata in codice e piano: contratti machine-readable, GUI senza affordance ingannevoli, restore metadata-only onesto, metriche operative affidabili e bound espliciti sul contesto orchestrato. Validazione: `cargo test --release` = 100/100, `python -m compileall gui src/eval_swarm.py` verde.

### Decisione 2026-03-07 — Future-model flexibility
- **Status:** DECIDED
- **Note:** Qwen3.5 viene trattato come primo caso guida per aprire un Model Abstraction Layer, metadata runtime-first, capability routing v2 e supporto previsto a driver esterni. Il cantiere e' strutturato come nuova tranche C22-C27 e fase 2.9 di roadmap.

### C23 — Prompt/token metadata runtime
- **Status:** DONE ✅
- **Data:** 2026-03-07
- **Note:** `prompting.rs`, `engine/tokenizer.rs`, `engine/lifecycle.rs`, `runtime.rs`, `commands/model.rs` e `commands/exec.rs` ora consumano metadata runtime per chat template, assistant preamble, stop markers e special tokens. Fallback legacy mantenuto e testato. Validazione: `cargo test --release` verde.

### C25 — Capability routing v2 osservabile
- **Status:** DONE ✅
- **Data:** 2026-03-07
- **Note:** `model_catalog.rs` espone routing recommendations spiegabili (`source`, `rationale`, score, metadata source/backend preference) e `gui/widgets/models.py` visualizza capability e razionale del routing. Validazione: `cargo test --release` = 105/105, `python3 -m compileall gui` verde.

### C26 — Driver plane esterno / model-driver resolution
- **Status:** DONE ✅
- **Data:** 2026-03-07
- **Note:** `backend.rs` introduce registry driver e policy di risoluzione modello -> driver; `engine/lifecycle.rs` carica il backend risolto; `model_catalog.rs` espone `resolved_backend` e stato del driver; `gui/widgets/models.py` mostra driver risolto, preferenza e stato. Validazione: `cargo test --release` = 109/109, `python3 -m compileall gui` verde.
