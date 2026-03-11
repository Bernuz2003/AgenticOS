# REPORT.md

Analisi del repository `AgenticOS` basata sul codice presente nel workspace il 11 marzo 2026.

## 1. Scopo del report

Questo report descrive in profondita' il sistema `AgenticOS` nella sua forma attuale, includendo:

- kernel Rust;
- protocollo di controllo TCP;
- crate condivisa `agentic-protocol`;
- GUI desktop Tauri `Agent Workspace` con frontend React/TypeScript e bridge Rust;
- componenti secondari del repository: schema protocollo, GUI legacy PySide6, utility Python, benchmark, documentazione.

L'obiettivo non e' riassumere l'intenzione del progetto, ma spiegare come funziona davvero oggi il codice, quali moduli esistono, come si parlano e quali confini architetturali emergono.

---

## 2. Visione d'insieme

`AgenticOS` e' un sistema local-first composto da due blocchi principali:

1. un kernel Rust single-node che espone un control plane TCP autenticato e un runtime di esecuzione per processi LLM;
2. una GUI desktop Tauri che usa quel control plane per osservare, avviare e controllare sessioni e orchestrazioni.

Il kernel gestisce:

- caricamento modello;
- spawning di processi agentici;
- generazione token in streaming;
- compattazione del contesto;
- scheduling per priorita' e quota;
- memoria logica a slot/pagine con swap asincrono;
- syscall/tool sandbox;
- orchestrazione DAG multi-task;
- checkpoint metadata-only.

La GUI Tauri gestisce:

- bootstrap dell'app desktop;
- bridge TCP autenticato verso il kernel;
- invio di comandi control-plane;
- stream dedicato per `EXEC`;
- stato frontend Lobby/Workspace;
- timeline live e fallback su `STATUS`.

---

## 3. Mappa del repository

### 3.1 Directory principali

| Percorso | Ruolo |
| --- | --- |
| `src/` | kernel Rust principale, oggi in un unico crate binario molto grande |
| `crates/agentic-protocol/` | crate Rust condivisa con opcode, framing header, envelope protocollo |
| `protocol/` | contratti JSON Schema versionati del control plane |
| `apps/agent-workspace/` | GUI desktop primaria: frontend React/Vite + backend Tauri Rust |
| `gui/` | GUI PySide6 legacy, ancora presente come fallback diagnostico |
| `agenticos_shared/` | utility Python condivise, soprattutto loader di runtime config |
| `models/` | modelli GGUF e tokenizer/metadati correlati |
| `workspace/` | token auth, checkpoint, swap, audit log e artefatti runtime locali |
| `reports/` | output benchmark/report |
| `docs/` | ADR e prompt documentali |

### 3.2 Osservazione architetturale importante

L'organizzazione attuale e' funzionale ma non ancora ottimale:

- il kernel vive quasi interamente dentro `src/` come crate monolitico;
- il workspace Cargo root include solo `crates/agentic-protocol`;
- `apps/agent-workspace/src-tauri` ha un `Cargo.toml` separato con `[workspace]` locale, quindi non e' integrato nel workspace principale;
- esiste gia' un primo tentativo di estrazione in `crates/`, ma la maggior parte del codice resta nel crate root.

Questo punto diventera' rilevante nella proposta finale di ristrutturazione.

---

## 4. Architettura logica del sistema

### 4.1 Blocchi runtime

```text
React UI
  -> Tauri invoke
  -> Bridge Rust desktop
  -> TCP control plane autenticato
  -> Kernel event loop (mio)
  -> runtime processi/model inference/syscall/memory/orchestrator
```

### 4.2 Modello di concorrenza del kernel

Il kernel non e' multi-threaded nel senso classico del controllo stato.

Il modello reale e':

- thread principale: possiede quasi tutto lo stato (`Kernel`);
- worker inference: riceve un `AgentProcess` posseduto, calcola un passo e lo restituisce;
- worker syscall: esegue tool/syscall fuori dal thread principale;
- worker swap: persiste payload di swap su disco.

Quindi:

- la mutazione dello stato del kernel resta centralizzata;
- i worker non condividono direttamente `Kernel`, `LLMEngine` o `NeuralMemory`;
- il pattern dominante e' checkout/checkin tramite canali `mpsc`.

### 4.3 Superfici di integrazione

Le interfacce tra sottosistemi piu' importanti sono:

- `agentic_protocol`: definizione del wire format condiviso;
- `src/protocol.rs`: policy di risposta del kernel;
- `KernelBridge` Tauri: client TCP persistente per control plane;
- `stream.rs` Tauri: stream dedicato per `EXEC`;
- `tool_registry` + `tools`: boundary tra runtime LLM e tooling esterno;
- `model_catalog` + `backend`: boundary tra modello richiesto e driver/backend effettivo.

---

## 5. Kernel Rust: entrypoint e lifecycle

### 5.1 `src/main.rs`

Il binario principale:

- inizializza `tracing_subscriber`;
- carica la config con `config::initialize()`;
- esegue cleanup di script temporanei dei tool;
- costruisce `Kernel::new(config)` e invoca `run()`.

### 5.2 `src/kernel/bootstrap.rs`

Costruisce il kernel completo:

- `mio::Poll`, `Events`, `TcpListener`;
- `NeuralMemory`;
- `ModelCatalog`;
- canali e worker inference;
- canali e worker syscall;
- generazione e scrittura del token auth in `workspace/.kernel_token`;
- inizializzazione metriche, scheduler, orchestrator e tool registry.

Questo file e' di fatto il composition root del kernel.

### 5.3 `src/kernel/server.rs`

Definisce:

- `struct Kernel`, contenitore di tutto lo stato runtime;
- `Kernel::run()`, l'event loop principale;
- accept dei client;
- read/write socket;
- invocazione periodica di `run_engine_tick()`;
- auto-checkpoint.

`Kernel` contiene direttamente:

- listener/server;
- client TCP con buffer;
- memoria;
- engine opzionale;
- model catalog;
- scheduler;
- orchestrator;
- canali worker;
- stato auth;
- tool registry;
- metriche.

Architetturalmente e' un "god object" intenzionale: tutto lo stato del kernel e' centralizzato qui.

---

## 6. Protocollo e transport

### 6.1 `crates/agentic-protocol`

Questa crate definisce il contratto wire minimo condiviso tra kernel e client Rust:

- `OpCode`;
- `CommandHeader`;
- `ProtocolEnvelope<T>`;
- `HelloRequest` e `HelloResponse`;
- `encode_command`;
- limiti `MAX_CONTENT_LENGTH`;
- `schema::*` con gli schema id logici.

Importante: questa crate non contiene tutti i payload dei comandi; contiene soprattutto wire framing, enum opcode ed envelope.

### 6.2 `src/protocol.rs`

Il modulo protocollo lato kernel aggiunge:

- serializzazione delle risposte `+OK` e `-ERR`;
- envelope JSON v1;
- gestione `HELLO`;
- policy `should_use_protocol_v1`;
- capability stable del kernel;
- `DATA raw` per streaming token.

Questa separazione significa che:

- `agentic-protocol` definisce il contratto base;
- il kernel aggiunge la logica concreta di risposta e negoziazione.

### 6.3 `src/transport/*`

Il transport layer gestisce il TCP raw:

- `client.rs`: stato del client, buffer, autenticazione, versione protocollo negoziata;
- `framing.rs`: parser a stati del formato `VERB agent_id content_length\n<payload>`;
- `io.rs`: lettura socket, parsing comandi, dispatch su `execute_command`, scrittura `output_buffer`.

### 6.4 Flusso di protocollo

Una connessione tipica funziona cosi':

1. il client apre il socket;
2. invia `AUTH` con token letto da `workspace/.kernel_token`;
3. opzionalmente invia `HELLO` per negoziare protocol version e capability;
4. invia comandi applicativi;
5. riceve:
   - `+OK CODE LEN\r\n<payload>`;
   - `-ERR CODE LEN\r\n<payload>`;
   - `DATA raw LEN\r\n<stream bytes>`.

---

## 7. Command layer del kernel

### 7.1 Dispatcher

`src/commands/mod.rs` e' il dispatcher centrale.

Responsabilita':

- auth gate;
- gestione `HELLO`;
- gestione `AUTH`;
- costruzione di `CommandContext`;
- dispatch a handler specializzati per opcode.

`CommandContext` incapsula tutte le dipendenze mutabili necessarie ai command handler:

- client;
- request id;
- memory;
- engine_state;
- model_catalog;
- scheduler;
- orchestrator;
- tool_registry;
- metriche;
- riferimenti a `in_flight` e `pending_kills`.

### 7.2 Gruppi di command handler

| File | Responsabilita' |
| --- | --- |
| `model.rs` | `LOAD`, `LIST_MODELS`, `SELECT_MODEL`, `MODEL_INFO`, `BACKEND_DIAG` |
| `exec.rs` | `EXEC` e policy di avvio processo |
| `status.rs` | `STATUS`, `STATUS <pid>`, `STATUS orch:<id>` |
| `process_cmd.rs` | `TERM`, `KILL` |
| `scheduler_cmd.rs` | priorita' e quote |
| `memory_cmd.rs` | `MEMW` |
| `checkpoint_cmd.rs` | `CHECKPOINT`, `RESTORE` |
| `orchestration_cmd.rs` | `ORCHESTRATE` |
| `tools_cmd.rs` | `LIST_TOOLS`, `REGISTER_TOOL`, `TOOL_INFO`, `UNREGISTER_TOOL` |
| `misc.rs` | `PING`, `SHUTDOWN`, `SET_GEN`, `GET_GEN` |

### 7.3 Osservazione

Il command layer oggi e' relativamente pulito: il parsing socket e' separato, ma il control plane vive ancora nello stesso crate del runtime e dipende direttamente da quasi tutti i sottosistemi.

---

## 8. Engine, backend e processo agentico

### 8.1 `src/engine/*`

`LLMEngine` e' il cuore della parte modello/processi.

Contiene:

- `master_model: Option<RuntimeModel>`;
- tokenizer;
- device;
- tabella `processes: HashMap<u64, AgentProcess>`;
- metadati modello;
- generation config;
- token EOS/EOT;
- family e backend attivo.

### 8.2 `src/backend/*`

Questo package definisce il boundary astratto verso i backend di inferenza.

Pezzi principali:

- `InferenceBackend` e `ContextSlotPersistence`;
- `RuntimeModel`, wrapper object-safe su backend;
- driver registry;
- driver resolution in base a family, architecture e backend preference;
- backend locali Candle:
  - `QuantizedLlamaBackend`;
  - `QuantizedQwen2Backend`;
- backend remoto opzionale:
  - `ExternalLlamaCppBackend` via HTTP;
- diagnostica endpoint remoto;
- adapter per request/response `llama.cpp`.

### 8.3 `src/process.rs`

`AgentProcess` rappresenta una sessione LLM eseguibile.

Contiene:

- modello duplicato o dedicato;
- tokenizer;
- stato (`Ready`, `Running`, `WaitingForMemory`, `WaitingForSyscall`, `Finished`);
- token stream completo;
- `index_pos` per l'avanzamento del forward;
- `max_tokens`;
- `syscall_buffer`;
- policy e stato del contesto;
- `context_slot_id`.

### 8.4 Gestione del contesto

Il processo ha una semantica di contesto molto piu' ricca di un semplice buffer token.

Esistono:

- `ContextPolicy`;
- `ContextState`;
- `ContextSegment`;
- `ContextStrategy`.

Le strategie implementate sono:

- `SlidingWindow`;
- `Summarize`;
- `Retrieve`.

Queste strategie agiscono prima del passo di inferenza, compattando il contesto quando il budget supera la soglia configurata.

### 8.5 `src/prompting.rs`

Gestisce:

- `PromptFamily`;
- `GenerationConfig`;
- template di system/user/interprocess message;
- stop markers family-aware e metadata-aware;
- rendering da chat template metadata, anche Jinja-like con `minijinja`.

In pratica e' il layer che adatta prompt, stop condition e injection al modello effettivo.

### 8.6 `src/policy/mod.rs`

Decide:

- workload class inferita o hintata da `EXEC`;
- policy di contesto per processo;
- generation defaults per family;
- scheduler quota defaults per workload.

Ha quindi il ruolo di policy derivation, non di execution.

---

## 9. Runtime loop e worker execution

### 9.1 `src/inference_worker.rs`

Il worker inference:

- riceve `InferenceCmd::Step { pid, process, eos_token_id, eot_token_id }`;
- esegue un solo passo di generazione su un processo posseduto;
- restituisce `InferenceResult::Token` o `InferenceResult::Error`.

Questo design evita lock condivisi sul modello processo.

### 9.2 `src/runtime/mod.rs`

`run_engine_tick()` coordina il runtime per ogni ciclo del kernel.

Ordine logico:

1. poll degli eventi di swap;
2. drain delle syscall completate;
3. drain dei risultati inference;
4. gestione processi finiti;
5. checkout dei processi attivi verso il worker inference;
6. avanzamento delle orchestrazioni.

### 9.3 `src/runtime/inference_results.rs`

Responsabilita':

- reinserire il processo restituito dal worker dentro `engine.processes`;
- pulire `in_flight`;
- aggiornare scheduler e checked_out metadata;
- inviare chunk `DATA raw` al client proprietario;
- aggiornare output dell'orchestrator;
- intercettare syscall `[[...]]`;
- far rispettare quota token.

### 9.4 `src/runtime/syscalls.rs`

Responsabilita':

- worker syscall dedicato;
- parsing del `syscall_buffer`;
- dispatch di syscall:
  - `SPAWN`;
  - `SEND`;
  - tool registrati;
- reiniezione del risultato nel processo con `inject_context`.

### 9.5 `src/runtime/orchestration.rs`

Responsabilita':

- identificare processi finiti e notificare il client con marker `[PROCESS_FINISHED ...]`;
- rilasciare risorse del processo;
- scegliere i PID attivi in scheduling order;
- fare checkout dei processi al worker inference;
- spawnare task orchestration dipendenti;
- killare task in fail-fast.

---

## 10. Memory subsystem

### 10.1 `src/memory/core.rs`

`NeuralMemory` e' una memoria logica a slot contestuali, non una RAM generica.

Concetti chiave:

- `ContextSlotId`;
- `slot_table`;
- mapping `pid -> slot`;
- pagine/blocchi fisici;
- LRU order;
- quota di token slot per PID;
- contatori di allocazione, swap, OOM.

### 10.2 Scrittura memoria

`MEMW` e i path interni scrivono raw bytes allineati a `f32` dentro lo slot del PID.

Se non c'e' spazio:

- prova eviction LRU;
- se non basta e swap async e' attivo, mette il payload in coda per swap;
- marca il PID come waiting.

### 10.3 `src/memory/eviction.rs`

Contiene:

- pulizia pagine di uno slot;
- aggiornamento LRU;
- selezione vittima;
- reclaim di blocchi fino a fit.

### 10.4 `src/memory/swap.rs` e `swap_io.rs`

Lo swap manager:

- prepara target `.tmp` e `.swap` dentro `workspace/swap`;
- persiste payload in worker dedicato;
- traccia PID in attesa;
- restituisce `SwapEvent` al runtime;
- prova respawn del worker in caso di crash.

### 10.5 Ripristino

Nel runtime:

- `memory.poll_swap_events()` restituisce eventi completati;
- `memory.restore_swapped_pid()` rilegge il payload e lo riscrive nello slot;
- il backend prova `engine.load_context_slot(...)` se supportato;
- il processo viene riportato `Ready` se era `WaitingForMemory`.

### 10.6 Significato architetturale

Questo sottosistema e' ancora "logical-first":

- il kernel ragiona gia' in termini di context slot persistibili;
- il supporto fisico dipende dal backend effettivo;
- Candle locale oggi usa un path compatibile ma non un vero KV cache persistence nativo come `llama.cpp`.

---

## 11. Scheduler

### 11.1 `src/scheduler.rs`

Lo scheduler non possiede i processi; possiede metadati per PID:

- priorita';
- quote;
- accounting;
- metadata restored;
- metadata checked-out.

### 11.2 Cosa governa

Governa:

- ordine di stepping dei processi attivi;
- limite massimo token;
- limite massimo syscall;
- stato osservabile anche quando il processo e' fuori dall'engine perche' in flight.

### 11.3 Priorita'

Livelli:

- `Low`;
- `Normal`;
- `High`;
- `Critical`.

### 11.4 Quote

Quote default per workload:

- `Fast`;
- `Code`;
- `Reasoning`;
- `General`.

Queste quote sono configurabili e aggiornabili via command handler.

---

## 12. Model catalog e routing

### 12.1 `src/model_catalog/*`

Questo package e' il sistema di discovery e selezione modello.

Responsabilita':

- cercare file `.gguf`;
- trovare `tokenizer.json`;
- leggere metadata da GGUF, tokenizer e sidecar `metadata.json`;
- inferire family;
- risolvere backend/driver preferito;
- produrre JSON per `LIST_MODELS` e `MODEL_INFO`;
- raccomandare un modello per workload.

### 12.2 Discovery

`discovery.rs`:

- esplora ricorsivamente `models/`;
- costruisce `ModelEntry`;
- calcola fingerprint del catalogo per refresh incrementale.

### 12.3 Metadata

`metadata.rs` fonde tre sorgenti:

- metadati nativi GGUF;
- metadati derivati dal tokenizer;
- metadata sidecar JSON.

Il risultato e' `ModelMetadata`, che puo' contenere:

- family;
- architecture;
- backend_preference;
- chat_template;
- assistant_preamble;
- special_tokens;
- stop_markers;
- capability scores.

### 12.4 Routing

`routing.rs` seleziona il modello:

- prima per capability score metadata-aware;
- poi per fallback su family;
- infine first available.

Questo meccanismo e' usato:

- da `LIST_MODELS` per mostrare raccomandazioni;
- da `EXEC` per eventuale auto-switch basato su workload.

---

## 13. Orchestrator

### 13.1 `src/orchestrator/*`

L'orchestrator implementa workflow DAG multi-task.

Input:

- `TaskGraphDef` JSON;
- lista task con `deps`;
- `FailurePolicy`.

Output:

- `SpawnRequest` per i task pronti.

### 13.2 Pipeline interna

1. valida grafo e topological order;
2. registra orchestrazione;
3. spawna task root;
4. accumula output dei task;
5. quando i task dipendenti hanno tutte le dipendenze completate, costruisce prompt che include gli output precedenti;
6. applica `FailFast` o `BestEffort`.

### 13.3 Stato

Per ogni task esistono stati:

- `Pending`;
- `Running { pid }`;
- `Completed`;
- `Failed { error }`;
- `Skipped`.

### 13.4 Integrazione con il runtime

L'orchestrator non esegue direttamente LLM.

Si appoggia a:

- runtime per spawn/kill reale dei processi;
- scheduler e memory attraverso `spawn_managed_process`;
- `STATUS orch:<id>` per osservabilita'.

---

## 14. Tooling e syscall sandbox

### 14.1 `src/tool_registry.rs`

Definisce registry tipizzato dei tool:

- descriptor;
- backend kind;
- source built-in o runtime;
- alias;
- schema input/output;
- enabled/dangerous/capabilities.

Tool built-in attuali:

- `python`;
- `write_file`;
- `read_file`;
- `list_files`;
- `calc`.

Sono previsti backend:

- `Host`;
- `Wasm`;
- `RemoteHttp`.

### 14.2 `src/tools/mod.rs`

E' il dispatcher reale delle syscall.

Responsabilita':

- parse di `[[TOOL:...]]` e alias compat;
- rate limiting;
- audit log;
- dispatch al runner corretto;
- enforcement della policy remota.

### 14.3 `src/tools/runner.rs`

Esegue concretamente:

- Python host o container;
- read/write file nel workspace;
- list files;
- `calc` come Python wrapper.

La sandbox e' pragmatica, non ancora hard isolation full-stack:

- modalita' `host`, `container`, `wasm`;
- fallback opzionale host;
- timeout tramite wrapper `timeout`;
- path guard sul workspace.

### 14.4 `src/tools/policy.rs`

Gestisce:

- config syscall;
- finestra di rate limiting;
- burst di errori che possono killare il processo.

### 14.5 `src/tools/audit.rs`

Scrive log JSONL in `workspace/syscall_audit.log`.

Questo log e' consumato dalla GUI Tauri per collegare tool call e tool result nella timeline.

---

## 15. Checkpoint, restore, config, errori e servizi

### 15.1 `src/checkpoint.rs`

Il checkpoint e' metadata-only.

Salva:

- timestamp;
- versione;
- active family;
- selected model;
- generation config;
- scheduler snapshot;
- process metadata;
- memory counters;
- kernel metrics.

Non salva:

- pesi modello;
- tensor data reali;
- buffer socket;
- processi vivi eseguibili.

### 15.2 Restore

`RESTORE`:

- richiede kernel idle;
- ricarica metadata scheduler/processi;
- azzera `engine_state`;
- ripristina selected model hint;
- registra processi restored come metadata, non processi vivi.

### 15.3 `src/config.rs`

La config arriva da:

- `agenticos.toml`;
- override env var.

Copre:

- network;
- protocol;
- paths;
- memory;
- context;
- checkpoint;
- auth;
- external llama.cpp;
- exec auto-switch;
- orchestrator;
- tools;
- generation profiles;
- scheduler quotas.

### 15.4 `src/errors.rs`

Esiste una gerarchia errori typed:

- `KernelError`;
- `MemoryError`;
- `EngineError`;
- `ProtocolError`;
- `CatalogError`;
- `OrchestratorError`.

Nota importante: il codice oggi usa ancora molte API string-based; la gerarchia typed esiste ma non e' ancora adottata end-to-end.

### 15.5 `src/services/*`

Contiene servizi trasversali:

- `model_runtime.rs`: attivazione transazionale del modello target;
- `process_runtime.rs`: spawn/kill/release dei processi gestendo insieme engine, memory e scheduler.

Questi servizi sono un primo passo verso boundary piu' chiari.

---

## 16. GUI Tauri: architettura generale

### 16.1 Struttura

La GUI primaria vive in `apps/agent-workspace/` e ha due parti:

- frontend React/Vite in `apps/agent-workspace/src/`;
- backend Tauri Rust in `apps/agent-workspace/src-tauri/src/`.

### 16.2 Responsabilita' della shell Tauri

La shell Tauri:

- espone comandi `#[tauri::command]` al frontend;
- mantiene bridge persistente TCP verso il kernel;
- mantiene timeline store per stream `EXEC`;
- decodifica payload protocollo;
- trasforma payload kernel in modelli UI stabili.

### 16.3 Responsabilita' del frontend React

Il frontend:

- gestisce pagine Lobby e Workspace;
- usa store Zustand per snapshot lobby e workspace;
- invoca comandi Tauri;
- renderizza timeline, telemetria contesto, orchestrazioni e catalogo modelli.

---

## 17. Backend Tauri Rust nel dettaglio

### 17.1 `src-tauri/src/lib.rs`

Registra i comandi Tauri:

- bootstrap;
- fetch lobby/workspace/timeline;
- list/select/load model;
- start session;
- orchestrate;
- ping;
- shutdown;
- protocol preview.

### 17.2 `app_state.rs`

`AppState` contiene:

- `workspace_root`;
- `kernel_addr`;
- `Arc<Mutex<KernelBridge>>`;
- `Arc<Mutex<TimelineStore>>`.

Quindi tutta la shell desktop condivide:

- un bridge control-plane;
- uno storage timeline live.

### 17.3 `commands/kernel.rs`

Ogni comando Tauri:

- prende lo state;
- usa `spawn_blocking`;
- chiama il bridge o lo stream layer;
- restituisce DTO serializzabili verso il frontend.

La scelta `spawn_blocking` evita di bloccare il runtime Tauri con I/O socket sync.

### 17.4 `kernel/protocol.rs`

E' il client protocollo lato Tauri.

Implementa:

- `send_command`;
- `read_single_frame`;
- `read_stream_frame`;
- decode envelope v1;
- decode errore;
- handshake `HELLO`.

### 17.5 `kernel/auth.rs`

Risoluzione path del token:

- `workspace/.kernel_token` sotto la root workspace del repo.

### 17.6 `kernel/client.rs`

`KernelBridge` e' il cuore del bridge control-plane.

Fa:

- apertura socket persistente;
- auth automatica;
- `HELLO`;
- invio comandi control plane;
- decode tipizzato dei payload;
- mappatura dei payload a snapshot frontend.

Metodi principali:

- `fetch_lobby_snapshot()`;
- `fetch_workspace_snapshot(pid)`;
- `orchestrate(payload)`;
- `ping()`;
- `list_models()`;
- `select_model()`;
- `load_model()`;
- `shutdown()`.

### 17.7 `kernel/stream.rs`

Questo modulo apre una connessione separata per `EXEC`.

Flusso:

1. connect;
2. auth;
3. `HELLO`;
4. invio `EXEC`;
5. lettura del frame iniziale con il PID;
6. spawn di un thread dedicato per seguire i `DATA raw`;
7. aggiornamento di `TimelineStore`.

In piu':

- parse dei segmenti assistant/thinking/tool/system;
- riconoscimento del marker `[PROCESS_FINISHED ...]`;
- fallback timeline sintetica quando non esiste stream live;
- merge con audit log tool result.

### 17.8 `kernel/audit.rs`

Legge `workspace/syscall_audit.log`, estrae le entry del PID e le rende disponibili alla timeline.

### 17.9 `kernel/mapping.rs`

Helper per umanizzare eventi runtime e costruire audit event lato UI.

### 17.10 `models/kernel.rs`

Contiene i DTO Rust che il backend Tauri espone al frontend.

Punto rilevante:

- questi modelli replicano payload che esistono gia' implicitamente nel kernel;
- non sono oggi condivisi via crate comune con il kernel;
- questa duplicazione e' uno dei candidati naturali a refactoring.

---

## 18. Frontend React nel dettaglio

### 18.1 Routing e shell

- `main.tsx`: bootstrap React;
- `App.tsx`: `RouterProvider`;
- `app/router.tsx`: `createHashRouter`;
- `app/layout.tsx`: shell globale con header, badge connessione, polling lobby ogni 10s.

### 18.2 Store

`sessions-store.ts`:

- tiene lo stato Lobby;
- normalizza status delle sessioni;
- usa `fetchLobbySnapshot()`.

`workspace-store.ts`:

- tiene snapshot `STATUS <pid>` e timeline;
- refresha separatamente workspace e timeline.

### 18.3 API layer

`lib/api.ts`:

- wrap di tutte le invoke Tauri;
- trasformazione snake_case -> camelCase;
- definizione dei tipi TS usati in UI.

### 18.4 Pagine

`LobbyPage`:

- mostra control plane status;
- catalogo modelli;
- select/load/shutdown;
- card delle sessioni;
- entry point per nuove sessioni.

`WorkspacePage`:

- usa `sessionId` route param;
- risolve PID;
- fa polling periodico di snapshot e timeline;
- mostra `TimelinePane` e `MindPanel`.

### 18.5 Componenti principali

`NewAgentCard`:

- invoca `startSession`;
- naviga direttamente alla workspace del PID creato.

`SessionCard`:

- rappresenta una sessione/pid dalla Lobby.

`TimelinePane`:

- renderizza user/assistant/thinking/tool call/tool result/system event;
- fa autoscroll;
- mostra fallback notice quando non esiste stream attach.

`MindPanel`:

- telemetria contesto;
- budget e compressions;
- retrieval hits;
- orchestrazione del task corrente;
- audit stream filtrabile.

---

## 19. Flussi end-to-end piu' importanti

### 19.1 Avvio kernel

`main.rs`
-> `config::initialize`
-> `Kernel::new`
-> bootstrap listener/memory/catalog/worker/token auth
-> `Kernel::run`

### 19.2 Connessione GUI

Frontend React
-> Tauri command
-> `KernelBridge::ensure_connection`
-> connect TCP
-> `AUTH`
-> `HELLO`
-> comandi control plane

### 19.3 Caricamento modello

GUI/TCP client
-> `LOAD`
-> `commands/model.rs`
-> `model_catalog.resolve_load_target`
-> `services/model_runtime::activate_model_target`
-> `LLMEngine::load_target`
-> backend resolution + tokenizer resolution

### 19.4 Esecuzione `EXEC`

GUI start session
-> stream dedicated socket
-> `EXEC`
-> `commands/exec.rs`
-> policy resolution
-> eventuale auto-switch modello
-> `spawn_managed_process`
-> engine spawn + memory register + scheduler register
-> runtime tick checkout
-> inference worker step
-> token/result back
-> `DATA raw` al client
-> eventuale syscall interception
-> marker `[PROCESS_FINISHED ...]`

### 19.5 Aggiornamento Workspace

Frontend poll
-> `fetch_workspace_snapshot`
-> `STATUS <pid>`
-> kernel `status.rs`
-> snapshot pid/context/quota/orch binding
-> opzionale `STATUS orch:<id>`
-> merge lato Tauri
-> render `MindPanel`

### 19.6 Timeline live

`start_session`
-> `stream.rs`
-> thread dedicato
-> accumulo chunk in `TimelineStore`
-> frontend poll `fetch_timeline_snapshot`
-> parse segmenti
-> merge con audit tool result

### 19.7 Orchestrazione

GUI/CLI
-> `ORCHESTRATE`
-> validazione DAG
-> spawn task root
-> runtime appende output task
-> `orchestrator.advance()`
-> spawn task dipendenti o kill fail-fast
-> osservabilita' via `STATUS orch:<id>`

### 19.8 Swap

`MEMW` o write slot
-> OOM
-> swap queue worker
-> file `.swap`
-> `SwapEvent`
-> restore payload
-> process ready

---

## 20. Componenti secondari del repository

### 20.1 `protocol/`

Contiene JSON Schema versionati.

Serve come:

- documentazione formale del control plane;
- base per validazione contratti;
- separazione tra wire/runtime e schema docs.

### 20.2 `gui/`

GUI PySide6 legacy.

Ruolo attuale:

- fallback diagnostico;
- condivisione dello stesso protocollo TCP;
- utile come piano B mentre la GUI Tauri si consolida.

### 20.3 `agenticos_shared/`

Package Python piccolo ma utile:

- carica runtime defaults da `agenticos.toml`;
- evita hardcode duplicato in script Python.

### 20.4 Utility Python in `src/`

- `client.py`: client TCP minimale;
- `eval_llama3.py`: benchmark/smoke evaluation orientata a un modello;
- `eval_swarm.py`: benchmark comparativo single model vs routing per capability.

### 20.5 `docs/`

Attualmente contiene:

- ADR sul tool registry;
- prompt documentali.

---

## 21. Legami tra i moduli

### 21.1 Dipendenze forti nel kernel

Le dipendenze piu' strette oggi sono:

- `commands` <-> quasi tutti i sottosistemi;
- `runtime` <-> `engine`, `memory`, `scheduler`, `orchestrator`, `tools`;
- `engine` <-> `backend`, `process`, `prompting`, `model_catalog`;
- `process` <-> `prompting`, `memory`, backend persistence;
- `tools` <-> `tool_registry`, config, backend http, workspace path;
- `model_catalog` <-> `backend` per driver resolution.

### 21.2 Dipendenze forti nel lato GUI

- frontend React dipende dai DTO esposti dalla shell Tauri;
- shell Tauri dipende dal wire protocol e dai payload kernel;
- `KernelBridge` dipende sia da protocol decoding sia da mapping dei payload in modelli UI;
- `stream.rs` e `audit.rs` sono accoppiati al formato runtime dei marker e al file audit del kernel.

### 21.3 Coupling da notare

I coupling piu' visibili oggi sono:

- il kernel root crate accorpa troppi domini;
- i payload `STATUS/LIST_MODELS/EXEC/...` non sono condivisi come crate tipizzata tra kernel e Tauri;
- il protocollo e' descritto in tre posti:
  - `crates/agentic-protocol`;
  - `protocol/schemas`;
  - modelli Tauri in `models/kernel.rs`.

---

## 22. Valutazione sintetica dell'architettura attuale

### 22.1 Punti forti

- modello di ownership chiaro del kernel;
- separazione tra event loop e worker CPU/I/O;
- boundaries gia' riconoscibili per memory, model catalog, tools e orchestrator;
- GUI Tauri abbastanza allineata al control plane reale;
- schema docs e crate protocollo gia' presenti;
- servizi `model_runtime` e `process_runtime` come primi boundary applicativi.

### 22.2 Limiti strutturali

- crate kernel troppo monolitico;
- workspace Cargo principale incompleto;
- duplicazione DTO tra kernel e GUI;
- `Kernel` come contenitore enorme con molte responsabilita';
- `commands` e `runtime` ancora molto centrali e accoppiati;
- confine fra "control plane", "runtime core" e "domain model" non ancora esplicito a livello crate.

---

## 23. Proposta di struttura repository consigliata per il kernel

### 23.1 Giudizio

La struttura attuale e' utilizzabile, ma non e' ottimale per crescita, testabilita' e riuso tra kernel e GUI.

La criticita' principale non e' il numero di file, ma il fatto che:

- il root package e' contemporaneamente binario, libreria implicita e dominio principale;
- il workspace non rappresenta davvero tutti i componenti Rust del sistema;
- i confini architetturali non sono espressi nella struttura dei crate.

### 23.2 Obiettivo della riorganizzazione

Separare chiaramente:

- wire protocol;
- DTO/control payload condivisi;
- kernel domain/runtime;
- binario di bootstrap;
- GUI Tauri come app del workspace, non come sottoworkspace isolato.

### 23.3 Struttura consigliata

```text
/
|- Cargo.toml                        # workspace only
|- Cargo.lock
|- agenticos.toml
|- /apps
|  |- /agent-workspace
|  |  |- package.json
|  |  |- /src
|  |  |- /src-tauri
|  |     |- Cargo.toml
|  |     |- /src
|- /crates
|  |- /agentic-protocol             # framing, opcode, envelope, hello
|  |- /agentic-control-models       # STATUS/EXEC/LIST_MODELS/... shared DTOs
|  |- /agentic-kernel               # library crate
|  |  |- /src
|  |  |  |- lib.rs
|  |  |  |- /control                # commands, transport, protocol responses, auth gate
|  |  |  |- /runtime                # kernel loop, workers, orchestration tick
|  |  |  |- /model                  # backend, engine, process, prompting, model catalog
|  |  |  |- /memory                 # allocator, swap, memory types
|  |  |  |- /tools                  # tool registry, syscall sandbox, audit
|  |  |  |- /state                  # scheduler, config, errors, metrics, checkpoint
|  |- /agentic-kernel-bin           # main.rs only
|- /protocol
|  |- /schemas
|- /gui                             # deprecated PySide fallback
|- /docs
|- /models
|- /workspace
|- /reports
```

### 23.4 Mapping consigliato dall'attuale struttura

| Attuale | Destinazione consigliata |
| --- | --- |
| `src/commands`, `src/transport`, `src/protocol`, `src/kernel/server.rs` | `crates/agentic-kernel/src/control/` |
| `src/runtime`, `src/inference_worker`, `src/services`, parti di `src/kernel/` | `crates/agentic-kernel/src/runtime/` |
| `src/backend`, `src/engine`, `src/process`, `src/prompting`, `src/policy`, `src/model_catalog` | `crates/agentic-kernel/src/model/` |
| `src/memory` | `crates/agentic-kernel/src/memory/` |
| `src/tools`, `src/tool_registry` | `crates/agentic-kernel/src/tools/` |
| `src/checkpoint`, `src/config`, `src/errors`, `src/scheduler` | `crates/agentic-kernel/src/state/` |
| `src/main.rs` | `crates/agentic-kernel-bin/src/main.rs` |

### 23.5 Raccomandazione pratica

Non consiglio di esplodere subito il kernel in troppi micro-crate separati.

La soluzione migliore, in questa fase, e':

1. trasformare la root in workspace puro;
2. creare un crate libreria `agentic-kernel`;
3. spostare il binario in `agentic-kernel-bin`;
4. aggiungere `agentic-control-models` per eliminare la duplicazione DTO con la GUI;
5. includere `apps/agent-workspace/src-tauri` nel workspace principale.

Questo da' benefici immediati senza introdurre eccessiva frammentazione.

### 23.6 Workspace Cargo consigliato

```toml
[workspace]
members = [
  "crates/agentic-protocol",
  "crates/agentic-control-models",
  "crates/agentic-kernel",
  "crates/agentic-kernel-bin",
  "apps/agent-workspace/src-tauri",
]

[workspace.package]
edition = "2021"
version = "0.5.0"
```

---

## 24. Conclusione

`AgenticOS` oggi e' gia' un sistema abbastanza articolato e coerente:

- il kernel e' un runtime event-driven con stato centralizzato e worker dedicati;
- il control plane TCP e' sufficientemente strutturato e gia' negoziabile;
- il model layer e' capace di routing, metadata merge e backend abstraction;
- il memory layer introduce slot logici, swap e osservabilita';
- la GUI Tauri e' gia' costruita attorno al concetto corretto di Lobby + Workspace + Timeline live.

La prossima evoluzione naturale non e' tanto aggiungere nuove feature, quanto rendere espliciti a livello di repository e crate i confini che nel codice esistono gia':

- protocollo;
- payload condivisi;
- kernel library;
- binario kernel;
- GUI Tauri.

Questa riorganizzazione renderebbe il progetto piu' semplice da mantenere, piu' testabile e meno dipendente da duplicazioni tra kernel e GUI.
