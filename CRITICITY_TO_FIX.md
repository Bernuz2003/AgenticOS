# AgenticOS — Criticità da Risolvere

Fonte: analisi completa del codebase del 6 marzo 2026.
Questo file è il piano operativo per risolvere ogni criticità prima di procedere con la Fase 3.

---

## Indice criticità

| # | Severità | Titolo | Moduli coinvolti | Stima |
|---|----------|--------|------------------|-------|
| C1 | 🔴 CRITICA | Inferenza bloccante nell'event loop | `runtime.rs`, `main.rs`, `engine/` | ~8-12h |
| C2 | 🔴 CRITICA | Incoerenza modello di concorrenza (`Rc` vs `Arc`) | `main.rs`, `runtime.rs`, `memory/` | ~4-6h |
| C3 | 🟠 ALTA | Nessuna autenticazione sul protocollo TCP | `transport/`, `protocol.rs`, `commands/` | ~3-4h |
| C4 | ✅ DONE | `execute_command` monolitico (720 righe, 11 parametri) | `commands/mod.rs` | ~3-4h |
| C5 | 🟠 ALTA | STATUS flat non strutturato → parsing regex fragile | `commands/mod.rs`, GUI `widgets/` | ~4-6h |
| C6 | 🟡 MEDIA | Metriche globali statiche (`OnceLock<Mutex>`) | `commands/metrics.rs`, `tools.rs` | ~2-3h |
| C7 | 🟡 MEDIA | Qwen model reload completo per ogni spawn | `backend.rs`, `engine/lifecycle.rs` | ~2-3h |
| C8 | 🟡 MEDIA | GUI: connessione TCP nuova per ogni richiesta | `gui/protocol_client.py` | ~2-3h |
| C9 | 🟡 MEDIA | GUI: thread spawn per ogni richiesta (no pool) | `gui/app.py` | ~1-2h |
| C10 | 🟡 MEDIA | GUI: HTML rebuild completo ad ogni token di streaming | `gui/widgets/chat.py` | ~2-3h |
| C11 | 🟡 MEDIA | GUI: N+1 query problem nella sezione Processes | `gui/widgets/processes.py`, `gui/app.py` | ~1-2h |
| C12 | ✅ DONE | Porta 6379 hardcoded (conflitto Redis) | `main.rs`, `gui/protocol_client.py` | ~1h |
| C13 | 🟢 BASSA | Nessun recovery per swap worker thread crash | `memory/swap.rs` | ~1-2h |
| C14 | ✅ DONE | Debito tecnico minore (dead_code, lingua mista, unwrap) | Vari | ~2h |

**Effort totale stimato: ~35-50h**

---

## C1 — Inferenza bloccante nell'event loop

### Problema

`run_engine_tick()` in `runtime.rs` chiama `engine.step_process(pid)` sincronamente per ogni PID attivo dentro l'event loop `mio`. Ogni step è una forward pass del modello (~150-250ms su CPU per Llama 8B Q4_K_M). Con N processi attivi il kernel è bloccato per N×step_time, durante i quali:

- Nessun nuovo client può connettersi
- Nessun comando (PING, STATUS, KILL) viene processato
- Il poll timeout di 5ms è irrilevante

### Impatto

Il kernel è di fatto inutilizzabile con più di 1-2 processi simultanei. L'orchestrazione DAG (M16) lancia sub-task in parallelo ma l'inferenza è serializzata.

### Piano di fix

**Approccio: offload dell'inferenza in un thread dedicato con communicazione via channel.**

```
Event Loop (mio)          Inference Thread
     │                          │
     │── SpawnCmd(pid,prompt) ──►│
     │                          │── step_process(pid) ──►
     │                          │   ... forward pass ...
     │◄── TokenReady(pid,text)──│
     │                          │
     │── KillCmd(pid) ─────────►│
```

#### Sub-task

1. **C1.1** Definire `InferenceCommand` enum: `Step(pid)`, `Spawn(pid, prompt, config)`, `Kill(pid)`, `Inject(pid, text)`, `Shutdown`.
2. **C1.2** Definire `InferenceEvent` enum: `TokenGenerated { pid, text, owner_id }`, `ProcessFinished(pid)`, `ProcessError { pid, error }`, `SpawnOk(pid)`, `SpawnFailed { error }`.
3. **C1.3** Creare `InferenceWorker` struct che possiede `LLMEngine` e gira in un `std::thread`. Riceve comandi da `mpsc::Receiver<InferenceCommand>`, emette eventi su `mpsc::Sender<InferenceEvent>`.
4. **C1.4** Nel loop principale, sostituire `run_engine_tick()` con: (a) drain `InferenceEvent` dal channel (non-blocking `try_recv`); (b) inviare comandi di step per i PID attivi.
5. **C1.5** Rimuovere `Arc<Mutex<Option<LLMEngine>>>` — l'engine vive solo nel worker thread, eliminando il lock dal path critico.
6. **C1.6** Aggiornare `execute_command` per inviare comandi al worker via channel invece di accedere all'engine direttamente.
7. **C1.7** Test: verificare che PING/STATUS rispondono entro 10ms anche durante inferenza attiva.

#### Rischi

- Il worker thread possiede l'engine in esclusiva → nessun accesso read dal main thread per STATUS. Soluzione: il worker invia snapshot periodici o su richiesta.
- La logica di orchestration advance dipende da `engine.list_finished_pids()` — deve essere ricostruita dalla coda eventi.

#### DoD

- [ ] Inferenza avviene in thread dedicato
- [ ] Event loop risponde a PING in <10ms sotto load
- [ ] Tutti i 94 test esistenti passano (adattati)
- [ ] Clippy pulito

---

## C2 — Incoerenza modello di concorrenza

### Problema

`NeuralMemory` è wrappato in `Rc<RefCell<>>` (single-thread only), mentre `LLMEngine` è in `Arc<Mutex<>>` (thread-safe). Questa incoerenza:

- Impedisce di muovere qualsiasi componente in un altro thread senza refactoring
- Rende `Rc<RefCell<NeuralMemory>>` non-`Send`, bloccando la migrazione a async o multi-thread
- L'`Arc<Mutex>` sull'engine è overhead inutile nel design single-threaded attuale

### Piano di fix

**Dipendenza: risolvere C1 prima.** Il modello di concorrenza dipende dall'architettura scelta per l'inferenza.

#### Sub-task

1. **C2.1** Con C1 risolto, l'engine non è più condiviso → rimuovere `Arc<Mutex<Option<LLMEngine>>>`.
2. **C2.2** La `NeuralMemory` resta nel main thread (acceduta da command handlers e dal drain degli eventi). Mantenerla come `Rc<RefCell<>>` è corretto se il main thread è l'unico ad accederla. In alternativa, se il worker thread deve accedere alla memoria (es. per MEMW durante inferenza), migrare a `Arc<Mutex<>>`.
3. **C2.3** Decisione architetturale: la memoria è accessibile solo dal main thread → `Rc<RefCell>` resta. Documentare la regola.
4. **C2.4** Passare `NeuralMemory` per `&mut` dove possibile invece del `Rc<RefCell>` pattern, riducendo il borrow overhead.

#### DoD

- [ ] Un solo pattern di concurrency per ogni componente, documentato
- [ ] Nessun `Arc<Mutex>` su componenti single-thread-only
- [ ] Nessun `Rc<RefCell>` su componenti multi-thread
- [ ] Compilazione e test verdi

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

- [ ] Auth obbligatorio per default
- [ ] Token generato al boot, salvato in file
- [ ] GUI legge e usa il token automaticamente
- [ ] Bypass con env var per test
- [ ] Suite test verde

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

- [ ] STATUS restituisce JSON valido
- [ ] GUI parsa con `json.loads`, senza regex
- [ ] Un unico `parse_status()` usato da tutti i widget
- [ ] Nessuna regressione funzionale

---

## C6 — Metriche globali statiche

### Problema

`metrics.rs` usa `static OnceLock<Mutex<MetricsState>>` per le metriche e `tools.rs` usa `static RATE_STATES` per il rate limiting. Questi singletons:

- Inquinano l'ambiente di test (stato condiviso tra test paralleli)
- Impediscono di avere istanze multiple del kernel (test, embedding)
- Rendono il ciclo di vita delle risorse implicito

### Piano di fix

1. **C6.1** Spostare `MetricsState` come campo di `Kernel` (non-static).
2. **C6.2** Passare `&mut MetricsState` (o un wrapper) ai command handler tramite `CommandContext` (vedi C4).
3. **C6.3** Per `RATE_STATES` in `tools.rs`: passare come parametro al `handle_syscall`, o come campo di `CommandContext`.
4. **C6.4** `metrics_start` (uptime timer): campo `started_at: Instant` in `Kernel`.

#### DoD

- [ ] Nessuna `static` mutabile per stato applicativo
- [ ] Test possono istanziare metriche indipendenti
- [ ] Compilazione e test verdi

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

- [ ] Qwen spawn senza reload, oppure guard con errore chiaro
- [ ] Documentato in ARCHITECTURE.md
- [ ] Test per il comportamento scelto

---

## C8 — GUI: connessione TCP nuova per ogni richiesta

### Problema

`ProtocolClient.send_once()` apre un nuovo `socket.socket()` per ogni richiesta. Con STATUS polling ogni 5s + audit ogni 0.5s + modelli ogni 15s → ~15+ connessioni/minuto.

### Piano di fix

1. **C8.1** Aggiungere `PersistentClient` class che mantiene un socket aperto con reconnect on error.
2. **C8.2** Thread-safety: il client è usato da thread multipli → `threading.Lock` per serializzare send/recv, oppure un singolo IO thread con coda di richieste.
3. **C8.3** Approccio consigliato: un singolo "protocol IO thread" con una `queue.Queue` di richieste. I caller fanno `queue.put(request)` e attendono un `Future`/`Event` per la risposta.
4. **C8.4** `exec_stream` mantiene il pattern connessione dedicata (streaming lungo).

#### DoD

- [ ] Un singolo socket per le richieste control-plane (non EXEC)
- [ ] Reconnect automatico su disconnect
- [ ] Thread-safe
- [ ] Nessuna regressione funzionale GUI

---

## C9 — GUI: thread spawn per ogni richiesta

### Problema

Ogni `_dispatch_control_request()` crea un `threading.Thread(daemon=True)`. Nessun pooling, nessun limit.

### Piano di fix

1. **C9.1** Creare un `ThreadPoolExecutor(max_workers=4)` come attributo di `MainWindow`.
2. **C9.2** Sostituire `threading.Thread(target=task).start()` con `self._executor.submit(task)`.
3. **C9.3** Cleanup in `closeEvent`: `self._executor.shutdown(wait=False)`.

**Nota**: se si implementa C8 con un IO thread dedicato, C9 diventa parzialmente risolto (le richieste control non creano più thread). Restano thread per EXEC stream e operazioni lunghe (LOAD).

#### DoD

- [ ] Max 4 thread concorrenti per richieste protocol
- [ ] `closeEvent` fa cleanup
- [ ] Nessuna regressione

---

## C10 — GUI: HTML rebuild completo ad ogni token

### Problema

`ChatSection._refresh_display()` chiama `self.chat_display.setHtml()` con l'**intero contenuto** ad ogni chunk di streaming. Con 50 bubble, questo ricrea il DOM HTML completo ~10-20 volte al secondo durante inferenza.

### Piano di fix

1. **C10.1** Usare `QTextCursor` per modificare solo l'ultimo bubble (append-only durante streaming).
2. **C10.2** Alternativa più semplice: **throttle** il rendering. Accumulare i chunk in buffer e renderizzare max ogni 200ms con un timer dedicato.
3. **C10.3** Al termine dello streaming (`finish_assistant_message`), fare un render completo finale una sola volta.

**Approccio consigliato: C10.2 (throttle) è più semplice e meno rischioso di C10.1.**

#### DoD

- [ ] Nessun `setHtml()` per ogni token durante streaming
- [ ] Refresh visivo ≤5 volte/secondo durante streaming
- [ ] Nessuna perdita di testo

---

## C11 — GUI: N+1 query per Processes

### Problema

`ProcessesSection.update_from_status()` emette `status_pid_requested` per ogni PID attivo, generando N richieste TCP aggiuntive ad ogni refresh.

### Piano di fix

**Dipendenza: C5 (STATUS JSON) risolve alla radice** — includendo i dati per-PID nel response JSON globale.

1. **C11.1** Con C5 risolto, il STATUS globale include `active_processes: [{ pid, workload, priority, tokens, ... }]`.
2. **C11.2** Rimuovere il loop `for pid in all_pids: self.status_pid_requested.emit(pid)`.
3. **C11.3** `update_from_status()` popola direttamente la tabella dal JSON.

#### DoD

- [ ] Zero richieste extra per popolare la tabella processi
- [ ] Tabella completa dopo un singolo STATUS
- [ ] Nessun `status_pid_requested` nel refresh periodico

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

## C13 — Swap worker thread senza recovery

### Problema

Il swap worker thread in `memory/swap.rs` è spawned una volta. Se panics, il channel si disconnette e `poll_events()` disabilita lo swap silenziosamente. Nessun retry, nessun allarme visibile.

### Piano di fix

1. **C13.1** In `poll_events()`, quando si rileva `TryRecvError::Disconnected`, emettere un `tracing::error!` + incrementare un contatore `swap_worker_crashes`.
2. **C13.2** Tentare un re-spawn automatico del worker (1 retry).
3. **C13.3** Il contatore è visibile in STATUS per diagnostica.

#### DoD

- [ ] Log esplicito al crash del worker
- [ ] Un tentativo di re-spawn
- [ ] Contatore visibile via STATUS

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
```

---

## Regole operative

1. **Una criticità alla volta.** Completare, testare, committare prima di iniziare la successiva.
2. **Test prima del fix.** Se la criticità non ha test di regressione, scriverli prima.
3. **Nessun cambio funzionale.** I fix sono puri refactoring/hardening — nessuna nuova feature.
4. **Aggiornare ROADMAP.md** con una nuova sezione "Fase 2.9 — Hardening criticità" a fix completato.
5. **Aggiornare ARCHITECTURE.md** se un fix modifica il design documentato.

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
