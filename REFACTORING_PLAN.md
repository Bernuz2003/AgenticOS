# Refactoring Plan

## Objective

Questo documento definisce il piano operativo del refactor architetturale approvato per AgenticOS.

Obiettivo del refactor:

- rendere il progetto piu' professionale e coerente a livello di workspace Rust;
- eliminare la duplicazione dei contratti tra kernel e GUI Tauri;
- tipizzare i boundary critici invece di affidarsi a payload ed errori string-based;
- ridurre l'accoppiamento interno del kernel senza fare refactor cosmetici;
- mantenere il kernel come demone TCP indipendente e rendere la GUI piu' event-driven.

## Decisioni Architetturali Vincolanti

- Il kernel resta un processo separato dalla GUI Tauri.
- Il transport embedded e' fuori scope.
- Il control plane continuera' a usare TCP locale/autenticato.
- L'ottimizzazione richiesta e' sul design del layer TCP e del modello di aggiornamento, non sulla sua rimozione.

## Priorita' Approvate

1. Unificare il workspace Rust.
2. Estrarre un crate condiviso per i DTO del control plane.
3. Tipizzare rigidamente gli errori di boundary.
4. Disaccoppiare internamente il kernel riducendo il `God Object`.
5. Rendere il lato GUI e observability piu' event-driven riducendo il polling.
6. Estrarre il kernel in un crate libreria dedicato.
7. Separare il bootstrap binario del kernel e rendere la root un workspace puro.
8. Riallineare il tree interno del kernel al dominio architetturale target e correggere path/test.

## Non Goals

- riscrivere subito tutto il kernel in micro-crate;
- eliminare il protocollo TCP;
- accorpare kernel e Tauri nello stesso processo;
- ridefinire i payload pubblici senza una fase di compatibilita' esplicita.

## Execution Tracker

- [x] Phase 1. Workspace Rust unificato
- [x] Phase 2. Crate condiviso `agentic-control-models`
- [x] Phase 3. Errori di boundary tipizzati
- [x] Phase 4. Facade e state partition del kernel
- [x] Phase 5. TCP event-driven e riduzione del polling GUI
- [x] Phase 6. Crate `agentic-kernel`
- [x] Phase 7. Crate `agentic-kernel-bin` e workspace root puro
- [x] Phase 8. Re-home del tree kernel e path hardening

## Phase 1. Workspace Rust Unificato

### Outcome

Portare `apps/agent-workspace/src-tauri` dentro il workspace Cargo root insieme al kernel e a `crates/agentic-protocol`.

### Motivazione

Finche' Tauri vive in un sottoworkspace separato:

- i refactor condivisi sono piu' fragili;
- i lockfile e i check possono divergere;
- l'estrazione di crate comuni e' piu' costosa;
- manca una vista unica del grafo Rust del progetto.

### Tasks

1. Aggiungere `apps/agent-workspace/src-tauri` a `[workspace].members` nel root `Cargo.toml`.
2. Rimuovere il `[workspace]` locale dal `Cargo.toml` di Tauri.
3. Verificare `cargo metadata` e `cargo check --workspace`.
4. Mantenere invariato, per ora, il crate root del kernel come package + workspace.

### Acceptance Criteria

- `cargo check --workspace` risolve correttamente sia kernel sia Tauri.
- esiste un solo workspace Rust operativo.
- i path dependency esistenti restano validi.

### Status

Completata in questo step.

## Phase 2. Shared Control Models

### Outcome

Creare `crates/agentic-control-models` come source of truth per i DTO scambiati tra kernel e Tauri.

### Status

Completata sul boundary Rust-Rust:

- `STATUS`, `STATUS <pid>`, `STATUS orch:<id>`;
- `LIST_MODELS`, `MODEL_INFO`;
- result payload di `EXEC`, `ORCHESTRATE`, `LOAD`, `SELECT_MODEL`, `PING`, `SHUTDOWN`;
- il bridge Tauri importa i DTO control-plane direttamente dal crate condiviso;
- i test del model catalog validano i payload contro i DTO condivisi invece di usare `serde_json::Value`.

Nota:

- restano tipi TypeScript e view model UI lato frontend, ma non fanno piu' parte della duplicazione Rust kernel <-> Tauri che questa fase doveva eliminare.

### Motivazione

La duplicazione attuale dei payload tra:

- kernel Rust;
- backend Tauri Rust;
- mapping frontend;

e' il punto di rottura piu' pericoloso dell'integrazione.

### Scope

Il nuovo crate conterra':

- DTO per `STATUS`, `STATUS <pid>`, `LIST_MODELS`, `MODEL_INFO`;
- DTO per `EXEC`, `ORCHESTRATE`, `LOAD`, `SELECT_MODEL`, `PING`, `SHUTDOWN`;
- tipi condivisi per model catalog, scheduler/quota snapshot, context snapshot, orchestration snapshot;
- eventuali enum di supporto direttamente usate a boundary.

### Tasks

1. Creare il nuovo crate in `crates/agentic-control-models`.
2. Spostare i modelli oggi duplicati in `apps/agent-workspace/src-tauri/src/models/kernel.rs`.
3. Adattare il kernel a serializzare direttamente i DTO condivisi.
4. Adattare Tauri a deserializzare gli stessi DTO senza mirror locali.
5. Aggiungere contract tests base sui payload critici.

### Acceptance Criteria

- nessun DTO control-plane resta duplicato tra kernel e Tauri;
- i payload pubblici piu' usati sono definiti una volta sola;
- i test falliscono se kernel e bridge divergono.

## Phase 3. Typed Boundary Errors

### Outcome

Rendere typed tutti gli errori che attraversano boundary di protocollo, command layer e bridge Tauri.

### Motivazione

Prima di separare i domini bisogna rendere affidabile il debugging tra moduli. Gli errori string-based possono ancora esistere internamente in alcuni path, ma non devono piu' essere il contratto tra componenti.

### Scope

Target iniziale:

- protocol decode/encode errors;
- auth/handshake errors;
- command validation errors;
- model selection/load errors;
- transport and bridge errors Tauri;
- mapping stabile verso codici errore del control plane.

### Tasks

1. Introdurre enum `thiserror` dedicate ai boundary.
2. Mappare gli errori verso codici stabili e payload coerenti.
3. Sostituire i `Err(String)` sui path pubblici piu' critici.
4. Allineare il bridge Tauri a questi errori tipizzati.
5. Aggiungere test per le conversioni error -> response.

### Acceptance Criteria

- gli handler pubblici non espongono piu' errori raw string-based come contratto primario;
- i log e le risposte hanno codici prevedibili;
- il bridge Tauri puo' distinguere classi di errore senza parsing fragile.

### Status

Completata per il perimetro operativo principale:

- `kernel/protocol.rs` usa `KernelBridgeError` tipizzato invece di `Result<_, String>`;
- `kernel/client.rs` propaga errori typed fino al confine dei comandi Tauri;
- la conversione a `String` avviene solo sul boundary IPC dei `#[tauri::command]`;
- aggiunti test mirati per mismatch di schema e decode degli errori kernel con codice stabile;
- introdotto `ControlErrorCode` in `agentic-protocol` come source of truth dei codici errore pubblici;
- migrati i command handler `AUTH`, `STATUS`, `LOAD`, `EXEC`, `process`, `misc`, `memory`, `checkpoint`, `orchestration`, `scheduler`, `tools` a codici errore tipizzati sul lato kernel.

## Phase 4. Kernel Decoupling

### Outcome

Ridurre il dominio di `Kernel` e `CommandContext` introducendo facade e partizioni di stato piu' strette.

### Motivazione

Spostare file in cartelle diverse non risolve il problema se ogni handler continua a dipendere dall'intero stato mutabile del kernel.

### Strategia

Prima si separano le responsabilita' a livello di API interne, poi eventualmente si valuta l'estrazione in crate dedicati.

### Prima Partizione Proposta

- `ControlPlaneState`
- `RuntimeState`
- `ModelState`
- `MemoryState`
- `ObservabilityState`

### Tasks

1. Ridurre `CommandContext` a capability specifiche per gruppo di comandi.
2. Introdurre facade/services per model lifecycle, process lifecycle, status snapshots, orchestration queries.
3. Spostare i command handler verso dipendenze piu' strette.
4. Ridurre i punti che richiedono accesso mutabile all'intero `Kernel`.
5. Preparare il terreno per il futuro split `agentic-kernel` / `agentic-kernel-bin`.

### Acceptance Criteria

- i comandi `model`, `status`, `process`, `tools`, `orchestration` non richiedono tutti la stessa vista completa del kernel;
- il compile-time ownership model diventa piu' locale;
- i servizi trasversali esistenti vengono consolidati invece di moltiplicare gli accessi diretti.

### Status

Completata sul command layer:

- introdotto `StatusCommandContext` come capability ristretta read-mostly;
- introdotto `ModelCommandContext` come capability ristretta per il model control;
- estratto `src/services/status_snapshot.rs` come servizio dedicato alla costruzione degli snapshot `STATUS`;
- `src/commands/status.rs` ora fa parsing + response, mentre la composizione dei dati e' fuori dal command handler;
- `src/commands/model.rs` non dipende piu' dall'intero `CommandContext`;
- introdotto `ProcessCommandContext` come capability ristretta per term/kill;
- introdotto `OrchestrationCommandContext` come capability ristretta per l'avvio delle DAG;
- estratti `src/services/process_control.rs` e `src/services/orchestration_runtime.rs` per spostare fuori dai command handler la logica di controllo processo e bootstrap orchestration.
- introdotti `ExecCommandContext`, `SchedulerCommandContext`, `ToolsCommandContext`, `MiscCommandContext`, `MemoryCommandContext`, `CheckpointCommandContext`;
- i command handler `exec`, `scheduler`, `tools`, `misc`, `memory`, `checkpoint` ora dipendono da view-context dedicati invece del contesto monolitico;
- il dispatch centrale resta l'unico punto che costruisce `CommandContext`, mentre gli handler ricevono solo le capability necessarie;
- verifica oggettiva: nessun handler in `src/commands` riceve piu' `&mut CommandContext<'_>` direttamente.

## Phase 5. TCP Event-Driven GUI

### Outcome

Mantenere TCP ma spostare GUI e bridge verso un modello osservabile push-first, riducendo il polling continuo.

### Motivazione

Per l'uso desktop attuale, la vera inefficienza non e' il loopback TCP in se', ma:

- polling ripetuto di `STATUS`;
- snapshot complete ricalcolate spesso;
- aggiornamenti UI che arrivano in ritardo o fuori ordine;
- doppio lavoro tra stream timeline e fetch periodici.

### Direzione Tecnica

Il kernel resta demone indipendente, ma il bridge Tauri deve diventare piu' simile a un subscriber TCP che a un poller.

### Tasks

1. Definire eventi di runtime stabili per PID/sessione/orchestrazione/modello.
2. Introdurre una subscription TCP dedicata per eventi osservabili.
3. Mantenere `STATUS` e snapshot come fallback e bootstrap, non come canale primario live.
4. Fare in modo che Tauri traduca gli eventi TCP in eventi UI consistenti.
5. Ridurre il polling frontend ai soli casi di recovery o cold start.

### Acceptance Criteria

- timeline e mind panel si aggiornano principalmente via eventi push;
- il polling periodico viene ridotto o relegato al fallback;
- il bridge gestisce reconnect e resume senza bloccare la GUI.

### Status

Completata con il nuovo flusso push-first:

- aggiunto `SUBSCRIBE` al control plane TCP e capability `event_stream_v1`;
- introdotti `KernelEvent` e `KernelEventEnvelope` nel crate condiviso `agentic-control-models`;
- il kernel accoda eventi di runtime/control-plane (`SessionStarted`, `TimelineChunk`, `WorkspaceChanged`, `LobbyChanged`, `SessionFinished`, `SessionErrored`, `ModelChanged`, `KernelShutdownRequested`) e li flush-a ai client sottoscritti dal main loop `mio`;
- il bridge Tauri avvia in `setup` una subscription persistente separata con reconnect automatico e emissione di eventi app-level;
- `TimelineStore` Tauri non dipende piu' da una dedicated `EXEC` streaming thread: `start_session` apre una connessione breve per ottenere il PID, mentre la live UI arriva dal subscriber globale;
- il frontend React ascolta `kernel://bridge_status`, `kernel://lobby_snapshot`, `kernel://workspace_snapshot`, `kernel://timeline_snapshot` e aggiorna gli store Zustand in push;
- rimossi i `setInterval` da `layout.tsx` e `workspace-page.tsx`;
- `fetch_lobby_snapshot`, `fetch_workspace_snapshot` e `fetch_timeline_snapshot` restano come bootstrap/manual recovery path, non come canale live primario.

## Phase 6. Extract `agentic-kernel`

### Outcome

Spostare il codice Rust del kernel dal package root a un crate libreria dedicato in `crates/agentic-kernel`.

### Motivazione

Finche' il kernel resta il package root:

- il repository non esprime davvero il boundary tra workspace e prodotto kernel;
- il refactor architetturale resta incompleto;
- il binario e la libreria restano accoppiati nella stessa root;
- il tree consigliato dal report non e' ancora reale.

### Tasks

1. Creare `crates/agentic-kernel/Cargo.toml`.
2. Spostare i moduli Rust del kernel dentro `crates/agentic-kernel/src/`.
3. Introdurre `crates/agentic-kernel/src/lib.rs` come crate root reale.
4. Lasciare fuori da questo move i file Python/utility non appartenenti al crate Rust.
5. Verificare che il crate compili e testi nello stesso workspace.

### Acceptance Criteria

- il kernel non vive piu' nel package root;
- tutti i moduli Rust del kernel sono importati dal crate `agentic-kernel`;
- `cargo check --workspace` continua a passare.

### Status

Completata:

- creato `crates/agentic-kernel` come crate libreria reale;
- spostati i moduli Rust del kernel dal package root a `crates/agentic-kernel/src/`;
- lasciati fuori dal move i file Python/utility presenti in `src/`;
- introdotto `crates/agentic-kernel/src/lib.rs` come entrypoint della libreria;
- `cargo check --workspace` e `cargo test -q -p agentic-kernel` verdi dopo il move.

## Phase 7. Extract `agentic-kernel-bin`

### Outcome

Creare un crate binario dedicato che dipende da `agentic-kernel` e trasformare la root in workspace puro.

### Motivazione

Questa fase completa la separazione libreria/binario che il report indicava come prerequisito per un repository professionale e mantenibile.

### Tasks

1. Creare `crates/agentic-kernel-bin/Cargo.toml`.
2. Spostare il bootstrap runtime nel bin crate.
3. Ridurre il root `Cargo.toml` a solo `[workspace]`.
4. Mantenere il nome operativo del binario kernel compatibile con l'attuale toolchain.

### Acceptance Criteria

- la root del repository non e' piu' un package Cargo compilabile;
- il kernel e' avviabile tramite il bin crate dedicato;
- il workspace include `agentic-kernel` e `agentic-kernel-bin`.

### Status

Completata:

- creato `crates/agentic-kernel-bin`;
- spostato il bootstrap runtime nel bin crate;
- trasformato il root `Cargo.toml` in workspace puro;
- mantenuto il nome operativo del binario `agentic_os_kernel`.

## Phase 8. Re-home Tree And Path Hardening

### Outcome

Allineare i path e, dove sensato, il tree interno del crate `agentic-kernel` alla struttura architetturale target del report.

### Motivazione

Il crate split da solo non basta: dopo lo spostamento vanno corretti i punti che assumono ancora il vecchio package root e preparato il tree interno verso `control/runtime/model/memory/tools/state`.

### Tasks

1. Correggere i path che dipendono da `CARGO_MANIFEST_DIR` o dalla vecchia root package.
2. Aggiornare test/schema lookup e path runtime sensibili.
3. Iniziare il re-home interno coerente con i domini del report, senza fare micro-crate prematuri.
4. Verificare i comandi `cargo check`, `cargo test` e build Tauri dopo il move.

### Acceptance Criteria

- nessun test/schema lookup dipende implicitamente dalla vecchia root package;
- il crate `agentic-kernel` e' pronto per un successivo riallineamento interno ai domini `control/runtime/model/memory/tools/state`;
- il workspace completo resta verde.

### Status

Completata:

- corretti i path dipendenti dal vecchio package root per config, schema lookup e catalog discovery;
- normalizzati i path runtime del kernel verso root repository per `agenticos.toml`, `models/`, `workspace/`, `checkpoint` e `swap`;
- adattati i test del transport layer e del catalog discovery al nuovo crate root;
- verifiche completate con `cargo check --workspace`, `cargo test -q -p agentic-kernel`, `cargo test -q -p agent-workspace` e `npm --prefix apps/agent-workspace run build`.

## Suggested Order Of Execution

1. Consolidare subito il workspace.
2. Estrarre i DTO condivisi piu' usati da `STATUS` e `LIST_MODELS`.
3. Tipizzare gli errori dei path di protocollo e bridge.
4. Ridurre `Kernel` tramite facade partendo dai command group meno rischiosi.
5. Rendere la GUI TCP push-first e rimuovere il polling periodico.
6. Estrarre `agentic-kernel`.
7. Estrarre `agentic-kernel-bin` e convertire la root in workspace puro.
8. Correggere path/test e re-home interno del tree kernel.

## Validation Strategy

- `cargo check --workspace` ad ogni fase strutturale;
- test mirati kernel e Tauri sui contratti condivisi;
- build frontend dopo i cambi bridge/API;
- regressione manuale su Lobby, Workspace, Timeline, Mind Panel;
- aggiornamento continuo di `REFACTORING_PLAN.md` come tracker operativo del refactor.

## Immediate Next Step

Le fasi approvate 1-8 sono ora chiuse. I prossimi lavori non sono piu' prerequisiti del refactor base, ma affinamenti successivi:

- riallineamento interno opzionale del tree `agentic-kernel/src/` verso i domini `control/runtime/model/memory/tools/state` senza introdurre micro-crate prematuri;
- contract test dedicati al canale `SUBSCRIBE` / `event_stream_v1`;
- estensione del push model a tutte le superfici di osservabilita' residue della GUI.
