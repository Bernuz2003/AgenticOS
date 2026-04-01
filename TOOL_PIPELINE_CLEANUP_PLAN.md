### M45) Canonical Turn Pipeline, Structured Invocation Events, and Conversation Persistence Redesign
**Status:** `IN_PROGRESS`

## Scope

Questa milestone congela il redesign della pipeline di output/tool per eliminare il coupling tra:

- transport streaming
- parsing control-flow
- persistenza conversazionale
- proiezione GUI

L'obiettivo non e' aggiungere un'altra patch, ma ripristinare un modello unico e coerente tra locale, remoto, runtime, storage e workspace bridge.

## Confirmed Current State

### 0. Bridge verification update

- La projection live Tauri e' ora coperta da una suite dedicata in `apps/agent-workspace/src-tauri/tests/live_timeline_bridge.rs`.
- Il bridge non deve piu' inferire tool/action dal raw assistant stream: i tool box live sono renderizzati solo da `KernelEvent::InvocationUpdated` e dai record storici `kind=invocation`.
- Il parser bridge continua a gestire solo segmentazione assistant/thinking; il parsing raw di `TOOL:` / `ACTION:` e' stato rimosso dal live/history projection.
- L'ispezione diretta del DB reale del `2026-03-31` mostra che le occorrenze `TOTOOLOL` ancora presenti sono due assistant message completi della sessione `sess-1-000002`, quindi dati realmente persistiti dal kernel e non testo ricostruito dal bridge.
- Le sessioni di probe piu' recenti (`sess-4-000003`, `sess-4-000005`) non hanno introdotto nuove righe `TOTOOLOL` in `session_messages`.

### 1. Transport locale

- Il backend `llama.cpp` oggi normalizza meglio chunk delta/cumulativi/overlapping.
- Questo livello deve restare limitato alla normalizzazione del transport.
- Non abbiamo ancora evidenza sufficiente per dichiarare chiuso end-to-end il problema delle invocation locali corrotte.
- L'osservazione del DB e degli audit mostra ancora invocation locali malformate o intercettate in modo improprio, quindi il problema non puo' essere considerato risolto solo perche' il transport e' stato normalizzato.

### 2. Dispatch tool e GUI

- Il runtime emette ora eventi strutturati di invocation.
- La GUI ha recuperato il rendering dei tool usando tali eventi invece del parsing del raw `TOOL:`.

### 3. Nuova regressione confermata: persistenza chunk-like

- In `crates/agentic-kernel/src/events.rs`, ogni `KernelEvent::TimelineChunk` viene ancora persistito subito come assistant message.
- La persistenza quindi riflette il trasporto live, non il modello conversazionale canonico.
- In `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`, il reader storico ricostruisce letteralmente quei record assistant.
- Risultato: dopo restore/replay la timeline storica esplode in mini-messaggi/chunk.

Questa regressione non e' casuale: deriva dal fatto che la verita' persistita e' ancora token/chunk-shaped invece di essere segment-shaped.

### 4. Evidenza diretta gia' osservata

- Nel DB reale `session_messages` contiene ancora centinaia di righe assistant minuscole, una per frammento/token.
- Negli audit e negli eventi persistiti compaiono invocation malformate come `TOOL:find_files per` e `TOOL:ask_human: rich`.
- Questo conferma che il problema corrente non e' solo di GUI o di history replay: c'e' ancora logica legacy che trasforma testo invalido in syscall.

## Discovery Requirement

Questa milestone richiede osservazione diretta dello stato reale del sistema prima e durante l'implementazione.

Obblighi:

- consultare direttamente il DB persistito
- guardare i record reali di `session_turns`, `session_messages`, `audit_events`
- confrontare output modello locale/remoto, output ricevuto dal kernel, output persistito e output proiettato in GUI
- non accettare spiegazioni su `TOTOOLOL:` o altri marker corrotti senza evidenza diretta da test, dump o dati persistiti

## Architectural Decision

### Canonical layers

Il sistema viene congelato in quattro livelli separati:

1. Transport / backend normalization
2. Kernel turn assembly + control parsing
3. Persisted conversation model
4. Bridge/GUI projection

Ogni bug emerso finora nasce da sconfinamenti tra questi livelli.

## Ownership By Layer

### 1. Transport / backend normalization

Responsabilita':

- convertire il transport provider-specific in delta testuali canonici
- deduplicare cumulative snapshots
- normalizzare overlap/partial replay

Non responsabilita':

- detection tool/action
- semantica GUI
- persistenza
- decisioni su visible text vs control text

Output canonico atteso:

- `AssistantDelta { pid/run_turn_handle, text_fragment }`

### 2. Kernel turn assembly + control parsing

Questo e' il solo owner della semantica del turno.

Responsabilita':

- accumulare testo assistant visibile
- mantenere il buffer di candidati control-flow
- rilevare `TOOL:` / `ACTION:` e marker parziali
- distinguere visible text da control text
- chiudere/aprire segmenti assistant
- emettere eventi strutturati di invocation
- sospendere/riprendere il processo
- reiniettare il risultato tool/action

Non responsabilita':

- normalizzazione SSE/provider
- rendering GUI
- persistenza chunk-by-chunk

Decisione chiave:

- deve esistere una sola state machine canonica kernel-side per il turno attivo

Shape concettuale della state machine:

- `visible_text_buffer`
- `control_candidate_buffer`
- `state`

Stati concettuali:

- `NormalText`
- `ControlPrefixCandidate`
- `ControlBody`
- `WaitingForDispatch`
- `ResumedAfterDispatch`
- `TurnComplete`

### 3. Persisted conversation model

Decisione netta:

- la persistenza canonica non deve essere token-based
- la persistenza canonica non deve essere raw streaming based

Unita' canoniche:

- `Session`
- `Run`
- `Turn`
- `TurnSegment`
- `InvocationEvent`
- `TurnFinish`

`TurnSegment` rappresenta solo segmenti semantici consolidati, non chunk live.

Esempio valido:

1. user prompt
2. assistant visible segment
3. invocation dispatched
4. invocation completed/failed
5. assistant visible segment
6. turn finished

Esempio non valido:

- un record assistant per token
- un record assistant per chunk SSE
- un solo blob assistant finale quando il turno e' semanticamente spezzato da invocation

### 4. Bridge / GUI projection

Responsabilita':

- proiettare live state e persisted state verso DTO GUI
- mostrare tool/action box da eventi strutturati
- mostrare testo assistant da delta live o segmenti persistiti

Non responsabilita':

- inferire tool/action dal raw text come fonte primaria
- fare parsing semantico del control flow

Regola:

- live e persisted devono esporre la stessa semantica, solo in tempi diversi

## Canonical Conversation Model

### Session

Conversazione logica.

### Run

Esecuzione runtime concreta collegata alla sessione.

### Turn

Un input utente e tutta la risposta costruita dal sistema.

### TurnSegment

Segmento assistant consolidato o altro segmento semantico del turno.

### InvocationEvent

Evento strutturato di lifecycle per tool/action:

- detected/dispatched
- completed
- failed
- killed

### TurnFinish

Marker finale del turno con esito e reason.

## Canonical Turn Flow

### 1. User input

- creare `Turn`
- persistere input utente
- avviare il run/process relativo

### 2. Backend emits deltas

- backend locale/remoto normalizza il transport
- il kernel riceve solo delta testuali canonici

### 3. Kernel turn state machine

- accumula visible text
- accumula eventuali prefissi di controllo
- non persiste chunk singoli
- emette delta live alla GUI

### 4. Invocation boundary

Quando la invocation diventa completa:

- chiudere il segmento assistant corrente
- persistere il segmento assistant consolidato
- persistere evento invocation strutturato
- emettere evento live strutturato
- mettere il processo in `WaitingForSyscall`
- dispatchare tool/action

### 5. Invocation result

- persistere evento di completion/failure
- reiniettare output nel contesto del processo
- riportare la state machine in `NormalText`

### 6. Turn finish

- persistere l'ultimo segmento assistant consolidato
- persistere `TurnFinish`
- chiudere il turn attivo

## Persistence Rules To Freeze

### Canonical truth

La tabella conversazionale deve contenere solo:

- user messages
- assistant segments consolidati
- invocation events strutturati
- finish/error markers

### Non-canonical debug data

Se serve debug fine-grained:

- usare un log o una tabella tecnica separata
- non usare quel log come fonte primaria della GUI storica

### Important consequence

- `KernelEvent::TimelineChunk` non deve piu' diventare automaticamente un assistant message persistito
- i chunk live sono meccanica di streaming, non entita' conversazionali

## Simplification Targets

### Source of truth

- `crates/agentic-kernel/src/invocation/text.rs` resta la sola sorgente canonica per detection/parsing testuale delle invocation

### Backend contract

- locale e remoto convergono sullo stesso contratto di delta canonici

### Runtime contract

- il runtime possiede da solo la decisione: visible text vs control text vs dispatch

### Storage contract

- storage persiste boundary semantici, non chunk di trasporto

### GUI contract

- GUI consuma eventi/segmenti strutturati, non raw parsing come fonte primaria

## Migration Plan

### Fase 1: Freeze the model

- confermare formalmente il modello `Session/Run/Turn/TurnSegment/InvocationEvent/TurnFinish`
- dichiarare `TimelineChunk` evento live-only
- congelare l'ownership di ciascun layer
- validare il modello contro i dati reali osservati nel DB e negli audit

### Fase 2: Discovery rigorosa e verifica dello stato reale

- osservare direttamente la struttura effettiva del DB persistito
- verificare il contenuto reale delle tabelle durante conversazione normale, tool dispatch e resume
- confrontare output locale, output remoto, output ricevuto dal kernel e output persistito
- usare DB, audit e test end-to-end come fonte primaria di verita'

### Fase 3: Add kernel turn assembler

- introdurre una struttura esplicita per il turno attivo
- bufferizzare in memoria il segmento assistant corrente
- committare solo ai boundary semantici

### Fase 4: Remove chunk persistence from event flush

- smettere di persistere `KernelEvent::TimelineChunk` come assistant message immediato
- persistere assistant segments solo quando il kernel chiude il segmento

### Fase 5: Fix invalid local invocation handling

- verificare con test reali se `llama-server` emette delta veri o snapshot cumulativi
- verificare se il kernel sta dispatchando invocation invalide che non dovrebbero mai lasciare il piano del visible text
- non considerare il bug `TOTOOLOL:` chiuso senza evidenza end-to-end

### Fase 6: Align live and history projections

- far derivare il seed storico dagli stessi segmenti/eventi del live model
- eliminare la frammentazione della timeline dopo restore

### Fase 7: Separate diagnostic chunk tracing

- se necessario, introdurre un log tecnico dedicato per chunk/token streaming
- mantenerlo fuori dal modello storico della conversazione

## Required Test Coverage

### Transport

- locale delta
- locale cumulativo
- locale overlap
- remoto marker spezzati

### Runtime semantics

- testo visibile bufferizzato e consolidato correttamente
- marker parziali `T/TO/TOO/TOOL/TOOL:` gestiti da una sola state machine
- dispatch corretto
- `WaitingForSyscall` vs `WaitingForInput`
- reinjection corretta

### Persistence

- nessun assistant message per chunk singolo
- assistant segments persistiti solo a boundary semantico
- ordering corretto tra assistant segment e invocation event
- restore storico identico alla semantica live consolidata
- osservazione diretta delle righe DB prima/dopo il redesign

### GUI / bridge

- tool box guidato da eventi strutturati
- history projection e live projection semanticamente allineate
- assenza di raw tool leak come prerequisito di rendering
- nessun parsing raw dei marker tool/action nel bridge come fonte di verita'

### Live tests

- test locali gated su `llama-server`
- test remoti gated su provider/API key

## Files Expected To Change In The Redesign

### Kernel

- `crates/agentic-kernel/src/backend/local/llamacpp.rs`
- `crates/agentic-kernel/src/runtime/output/*`
- `crates/agentic-kernel/src/runtime/syscalls/*`
- `crates/agentic-kernel/src/events.rs`
- `crates/agentic-kernel/src/storage/conversation/*`
- eventuale nuovo modulo esplicito di turn assembly / segment buffering

### Workspace bridge

- `apps/agent-workspace/src-tauri/src/kernel/events.rs`
- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/*`
- `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`

### Tests

- `crates/agentic-kernel/tests/e2e/*`
- eventuali test storage/runtime mirati ai boundary semantici

## Deliverables

### 1. Piano operativo dettagliato

- questo documento M45 e i suoi aggiornamenti durante l'implementazione

### 2. Implementazione del redesign

- coerente con il modello `TurnSegment` / `InvocationEvent` / `TurnFinish`

### 3. Test end-to-end

- in folder dedicato
- usati realmente per validare persistenza, resume, dispatch e GUI contract

### 4. Validazione finale

- locale
- remoto
- resume
- persistenza
- GUI/bridge contract

## Explicit Non-Goals

- non reintrodurre parsing tool nella GUI
- non duplicare detection tra backend e runtime
- non usare il PID come identita' semantica della conversazione
- non salvare ogni chunk live come message canonico
- non introdurre una seconda pipeline storica separata da quella live

## Acceptance Criteria

La milestone e' completata quando:

- il backend locale continua a non corrompere `TOOL:`
- il runtime resta l'unico owner del parsing control-flow
- la persistenza canonica e' segment-based
- il restore storico non mostra piu' un messaggio per chunk/token
- la GUI visualizza i tool tramite eventi strutturati
- live e history proiettano la stessa semantica di turno
