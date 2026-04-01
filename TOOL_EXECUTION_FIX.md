# Iterazione fix proposta

## Decisione

Questa iterazione **non va implementata letteralmente come scritta nella bozza originaria**.

La direzione di fondo e' corretta:

- la semantica delle tool/action invocation deve restare kernel-owned;
- la GUI non deve inferire i tool box dal raw assistant text;
- live state, persisted history e audit devono restare distinti.

Pero' la bozza attuale parte da una fotografia del repository che **non e' piu' vera**.  
Se applicata alla lettera, spinge verso una reingegnerizzazione piu' larga del necessario e rischia di duplicare responsabilita' gia' presenti nella pipeline canonica.

La scelta professionale per questo progetto e' quindi:

- **non rifondare** il modello di tool execution;
- **correggere la projection** della pipeline gia' esistente;
- **consolidare** una sola invocation visibile per `invocation_id` nello storico e nel live seed;
- verificare come punto secondario il binding live su PID/sessione per evitare contaminazioni fra sessioni.

---

# Stato reale del progetto osservato nei sorgenti

## 1. Tool e action sono gia' kernel-owned

Nel tree corrente:

- il parsing `TOOL:` / `ACTION:` e' gia' kernel-side in `crates/agentic-kernel/src/runtime/output/assistant_output.rs`
- l'assembly dei segmenti assistant/thinking/syscall e' gia' in `crates/agentic-kernel/src/runtime/output/turn_assembly.rs`
- il lifecycle delle invocation e' gia' esposto come evento strutturato tramite `KernelEvent::InvocationUpdated`

Quindi il problema non e' piu' "spostare la semantica dal bridge al kernel": questo passaggio e' gia' stato fatto.

## 2. La persistenza non e' piu' raw-chunk owned

In `crates/agentic-kernel/src/events.rs`:

- `KernelEvent::TimelineSegment` non viene persistito direttamente come chat raw da reinterpretare
- il kernel drena i segmenti strutturati da `TurnAssemblyStore`
- i segmenti assistant vengono persistiti con `append_assistant_segment(...)`
- le invocation vengono persistite come `system kind=invocation`

Questo contraddice la parte centrale della bozza che parla ancora di:

- `KernelEvent::TimelineChunk`
- `append_assistant_message(turn_id, text)` come meccanismo principale del problema

Nel codice corrente questo non e' piu' il path canonico del fix.

## 3. La history Tauri usa gia' invocation strutturate

In `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`:

- lo storico legge `system kind=invocation`
- deserializza `InvocationEvent`
- costruisce `TimelineItemKind::ToolCall` / `ActionCall`

Quindi lo storico non sta piu' "inventando" il tool box partendo dalla stringa `TOOL:` dentro un assistant message.

## 4. La live timeline usa gia' `InvocationUpdated`

In:

- `apps/agent-workspace/src-tauri/src/kernel/events.rs`
- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/store.rs`
- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/snapshot.rs`

la live timeline aggiorna i tool box tramite:

- `KernelEvent::InvocationUpdated`
- `store.upsert_invocation(...)`

Anche qui il bug non nasce dal raw assistant parsing come owner semantico principale.

## 5. Il replay/resume e' gia' protetto dal leakage delle invocation

In `crates/agentic-kernel/src/commands/process/resume.rs`:

- vengono ricaricati tutti i `StoredReplayMessage`
- ma il prompt di resume reimmette solo:
  - `user`
  - `assistant` con `kind != "thinking"`
- i `system/invocation` non vengono replayati al modello

Questa e' una garanzia importante gia' presente oggi.

---

# Critica tecnica della bozza originaria

## Corretta

- Chiede un solo owner semantico per le invocation: corretto.
- Chiede che la GUI non derivi i tool box dal raw assistant text: corretto.
- Chiede separazione tra live, history e audit: corretto.
- Chiede di eliminare box sporchi e stati `dispatching` residui: corretto.

## Da correggere

### 1. Parte da file e flussi ormai superati

La bozza cita come centro del problema:

- `KernelEvent::TimelineChunk`
- `storage.append_assistant_message(...)`
- `parse_stream_segments(...)`

Nel repository corrente il path canonico non e' piu' quello.

### 2. Diagnostica male il bug principale attuale

Il problema piu' plausibile, leggendo il codice reale, e' un altro:

- ogni `InvocationUpdated` viene **appeso** in storage come nuovo `system/invocation`
- la projection storica e il live seed li **ripercorrono uno per uno**
- lo stesso `invocation_id` puo' quindi apparire piu' volte:
  - prima come `dispatching`
  - poi come `completed` o `failed`

Questo produce:

- box duplicati o sporchi
- stati intermedi residui visibili
- chiavi duplicate lato React (`id` basato su `invocation_id`)

Questa e' una root cause molto piu' aderente all'immagine del bug e allo stato reale del codice.

### 3. Propone una migrazione architetturale piu' ampia del necessario

La bozza spinge verso:

- nuova rappresentazione canonica
- possibile revisione pesante della persistence
- rimozione quasi totale del modello attuale

Per questo repository oggi e' eccessivo.  
La pipeline strutturata esiste gia'; il fix professionale e' **chiudere il gap di projection**, non aprire una seconda rifondazione.

### 4. Non distingue abbastanza bene tra append-only lifecycle log e UI projection

Il fatto che storage tenga piu' `InvocationUpdated` non e' per forza sbagliato:

- puo' funzionare come log append-only del lifecycle
- il problema nasce se la GUI/history lo tratta come conversation truth gia' pronta al rendering

La correzione giusta non e' necessariamente cambiare subito il formato persistito.  
Spesso basta rendere canonica la projection:

- una sola invocation visibile per `invocation_id`
- con stato finale piu' recente
- mantenendo la posizione semantica del primo dispatch

### 5. Trascura un possibile rischio secondario nel live cache

In `apps/agent-workspace/src-tauri/src/kernel/composer/workspace.rs` il path:

- `ensure_live_timeline_for_session_pid(...)`

fa short-circuit su `store.has_pid(pid)` senza verificare sempre che quel PID appartenga ancora alla stessa sessione.

Questo va verificato come rischio di contaminazione live, specialmente nei casi di attach/rebind.

Non e' detto che sia la root cause primaria dello screenshot, ma e' un punto reale da controllare.

---

# Decisione architetturale corretta per questo progetto

## Regola 1. Non reintrodurre il raw parsing come centro del fix

La semantica delle invocation e' gia':

- kernel-side
- event-based
- persisted come `InvocationEvent`

Il fix non deve riportare ownership nel bridge o in parser storico.

## Regola 2. Non cambiare schema o modello persistito senza necessita'

Per questa iterazione non serve:

- nuova tabella dedicata
- migrazione ampia della timeline
- nuovo modello parallelo di tool execution

Il path piu' professionale e':

- mantenere `system kind=invocation` come log strutturato del lifecycle
- correggere la projection storica/live seed

## Regola 3. La conversation projection deve consolidare per `invocation_id`

Per ogni turno, la timeline visibile deve mostrare:

- una sola entry per `invocation_id`
- posizionata dove la invocation e' comparsa la prima volta
- aggiornata con lo stato piu' recente (`completed` / `failed` / `killed`)

Questo evita:

- box `dispatching` fossilizzati
- duplicazioni spurie
- chiavi duplicate lato UI

## Regola 4. Audit e UI non sono la stessa cosa

Se in futuro serve audit completo del lifecycle:

- lo si tiene nel log append-only
- o in `audit_events`

Ma la chat/workspace deve renderizzare la projection canonica, non l'intero journal tecnico.

---

# Piano implementativo corretto

## Fase 1. Consolidamento storico per invocation

In `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`:

- raggruppare i messaggi di turno in ordine
- quando compaiono piu' `system/invocation` con lo stesso `invocation_id`
  - non aggiungere un nuovo box
  - aggiornare quello gia' presente con lo stato piu' recente

Vincolo:

- la posizione visiva resta quella del primo dispatch

## Fase 2. Consolidamento del live seed da SQLite

Lo stesso criterio va applicato a `load_timeline_seed(...)`, altrimenti:

- aprendo o riattaccando una sessione live seedata da history
- il live cache reimporta di nuovo `dispatching` + `completed` come due messaggi distinti

## Fase 3. Verifica del binding live su PID/sessione

Controllare il path:

- `ensure_live_timeline_for_session_pid(...)`

per assicurarsi che un PID gia' presente nello store non faccia short-circuit improprio se la sessione non coincide.

Questo e' un controllo architetturale corretto, ma separato dal consolidamento delle invocation.

## Fase 4. Test mirati

Servono test che coprano almeno:

- uno stesso `invocation_id` persistito prima come `dispatched` e poi come `completed`
- ricostruzione storica con una sola entry visibile
- live seed con una sola invocation consolidata
- assenza di box tool duplicati o residui nello storico

---

# Criteri di successo realistici

La milestone e' davvero chiusa quando:

- una invocation storica appare una sola volta nella timeline visibile
- se esiste uno stato finale persistito, non resta visibile il vecchio `dispatching`
- il live seed da SQLite non reintroduce duplicati della stessa invocation
- la GUI continua a usare dati strutturati, non raw parsing storico
- il codice resta piu' semplice del piano originario, non piu' complesso

---

# Sintesi finale

La bozza originale ha una buona intenzione architetturale, ma **diagnostica male il repository attuale**.

Nel progetto di oggi:

- il kernel possiede gia' la semantica delle invocation
- la persistence e' gia' strutturata
- la GUI non dovrebbe essere rifatta da zero

Il fix piu' professionale non e' "reinventare il modello Tool Execution".  
Il fix professionale e' **consolidare correttamente la projection della semantica gia' esistente** e impedire che il journal del lifecycle venga mostrato come se fosse una chat canonica gia' pronta.
