# Iterazione fix proposta

## Decisione

Questa iterazione **non va implementata esattamente come formulata nella bozza iniziale**.

La direzione di fondo e' corretta:

- la semantica del thinking deve essere kernel-owned
- la GUI non deve inferire il meaning del raw assistant text
- la history non deve dipendere dal riparsing di `<think>...</think>`

Pero' la bozza originaria non e' abbastanza allineata allo stato reale del repository e, se applicata alla lettera, rischia di introdurre una seconda semantica parallela invece di estendere la pipeline canonica gia' presente.

L'implementazione corretta deve quindi partire da una **revisione architetturale del piano**, non da una esecuzione cieca della bozza.

---

# Stato reale del progetto osservato nei sorgenti

## 1. Tool e action non sono piu' bridge-owned

Nel tree corrente:

- il parsing `TOOL:` / `ACTION:` e' gia' kernel-side in `crates/agentic-kernel/src/runtime/output/assistant_output.rs`
- l'assembly del turno e delle boundary assistant/syscall e' gia' in `crates/agentic-kernel/src/runtime/output/turn_assembly.rs`
- il lifecycle delle invocation e' gia' modellato come evento strutturato tramite `KernelEvent::InvocationUpdated`

Quindi la proposta non deve "risolvere thinking" ricreando da zero il modello del turno: deve **estendere la turn assembly esistente**.

## 2. Il bridge continua invece a possedere la semantica del thinking

Oggi il thinking viene ancora inferito bridge-side tramite parsing del raw assistant text in:

- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/parser.rs`
- `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`

Questo e' il vero gap ancora aperto.

## 3. La GUI e' gia' quasi allineata

La GUI ha gia' un rendering dedicato per il thinking in:

- `apps/agent-workspace/src/components/workspace/timeline-pane/message-list.tsx`

Il problema non e' il widget UI, ma il fatto che il widget si appoggia ancora a segmentazione derivata dal raw text.

## 4. La persistenza assistant non e' completamente "raw chunk owned"

Nel codice attuale:

- `KernelEvent::TimelineChunk` non viene persistito direttamente
- il kernel accumula output assistant in `TurnAssemblyStore`
- la persistenza avviene ai boundary tramite `flush_pending_assistant_segment(...)` in `crates/agentic-kernel/src/events.rs`

Questo significa che la bozza originaria sovrastima il problema lato persistenza: il repository e' gia' andato oltre una persistenza puramente chunk-by-chunk del visible text.

Il problema vero e' che questa persistenza e' oggi segmentata solo per:

- `assistant visible text`
- `invocation lifecycle`

ma **non** per `thinking`.

## 5. Il backend locale ignora ancora `reasoning_content`

In:

- `crates/agentic-kernel/src/backend/local/remote_adapter.rs`

`reasoning_content` viene decodificato ma scartato dal flusso canonico. Quindi oggi il sistema:

- sa ricevere reasoning sidecar dal provider/backend
- ma non ha ancora un canale semantico canonico per portarlo fino a runtime, storage e GUI

## 6. Il replay/resume e' un vincolo architetturale reale

In:

- `crates/agentic-kernel/src/commands/process/resume.rs`

la ricostruzione del prompt di resume concatena i messaggi `assistant` persistiti.

Questa e' la criticita' principale assente nella bozza iniziale: se persistiamo il thinking come assistant text senza una policy chiara, rischiamo di:

- reiniettare reasoning interno nel prompt di replay
- alterare il comportamento del modello nelle turn successive
- rendere il resume dipendente da materiale che dovrebbe essere solo UI/history facing

Quindi il thinking deve essere **persistito, ma non replayato automaticamente al modello**.

## 7. Gli step locali a chunk piccoli hanno un bug di continuita' del thinking

Questa criticita' emerge soprattutto sui backend locali resident (`external-llamacpp`, inclusi i modelli Qwen serviti via `llama-server`) quando la generazione viene spezzata in molti step piccoli.

Scenario tipico:

- `chunk_tokens` basso per avere scheduling granulare
- modello che emette reasoning inline con `<think>...`
- step che termina mentre il blocco `<think>` e' ancora aperto

Nel codice attuale:

- il parser/assembly kernel-side sa segmentare correttamente visible text e thinking
- ma il prompt di continuazione del processo viene ricostruito usando solo il testo assistant "visibile" consolidato
- quindi il raw assistant text realmente emesso dal modello durante il turno **non** viene preservato integralmente se contiene un `<think>` aperto

Effetto:

- al passo successivo il modello non vede piu' una continuazione fedele del proprio output
- puo' ripartire dal prefisso gia' prodotto
- il kernel, che e' ancora semanticamente "dentro" il blocco thinking, interpreta il nuovo prefisso come nuovo thinking text
- in GUI questo appare come ripetizione patologica del tipo `This is a classic riddle<think> ...`

Questo bug **non** si risolve in modo professionale:

- alzando semplicemente `chunk_tokens`
- disabilitando il thinking
- sperando che il modello chiuda sempre il blocco `<think>` entro il boundary di step

Serve invece una separazione architetturale esplicita fra:

- `display/persisted text`: visible text + thinking segmentato per storage/history/UI
- `continuation text`: buffer effimero di continuazione del turno assistant, fedele al raw output del modello, incluso un eventuale `<think>` ancora aperto

Il `continuation text` deve:

- essere usato solo per proseguire la generazione locale nel turno corrente
- non essere persistito come history canonica
- non essere reiniettato nel replay/resume storico
- essere scartato alla chiusura reale del turno assistant

Questo e' il fix strutturale necessario per supportare correttamente:

- `chunk_tokens` bassi
- scheduling granulare
- reasoning inline reale
- storage/history puliti e semanticamente corretti

---

# Critica della bozza iniziale

## Corretta

- Chiede un solo owner semantico per il thinking: corretto.
- Chiede che GUI e bridge smettano di inferire il thinking dal raw text come fonte primaria: corretto.
- Chiede convergenza live/persisted: corretto.
- Chiede supporto sia a `<think>...</think>` sia a `reasoning_content`: corretto.

## Da correggere

### 1. Parte da file e responsabilita' in parte superati

La bozza cita `apps/agent-workspace/src-tauri/src/kernel/stream.rs` come punto principale del problema, ma nel tree corrente il parsing thinking rilevante e' in:

- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/parser.rs`
- `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`

### 2. Rischia di introdurre un secondo modello del turno

Il progetto ha gia' una turn assembly kernel-side per visible text + syscall. La scelta professionale non e' aggiungere un nuovo parser thinking separato, ma estendere quello esistente.

### 3. Usa un modello `invocation` / `invocation_result` non allineato allo stato attuale

Il repository oggi usa gia' un modello coerente basato su:

- `InvocationEvent`
- `InvocationStatus::{Dispatched, Completed, Failed, Killed}`

Per questa iterazione non c'e' bisogno di introdurre un secondo asse semantico separato chiamato `invocation_result`. Basta continuare a usare l'evento strutturato esistente.

### 4. Non definisce la policy di replay

Questo e' il punto piu' importante mancante. Il thinking va reso disponibile a:

- live timeline
- history
- resume visuale

ma non deve diventare automaticamente parte del prompt ricostruito per il modello.

### 5. Non prevede una strategia di compatibilita' per i dati legacy

Esistono gia' assistant message storici con `<think>...</think>` embedded. Finche' non si migra tutto lo storico, il parser bridge-side puo' restare solo come:

- fallback legacy
- path di compatibilita'

ma non come owner semantico primario.

### 6. Non chiarisce che `reasoning_content` e inline thinking sono due ingressi diversi

Best practice:

- inline `<think>...</think>` = compatibilita' con modelli che mischiano reasoning e visible text
- `reasoning_content` separato = canale preferibile quando il provider lo offre

Il kernel deve unificare questi due ingressi in una semantica comune, ma non deve trattarli come se fossero lo stesso transport.

---

# Decisione architetturale corretta per questo progetto

## Regola 1. Estendere la turn assembly esistente

La semantica del thinking deve nascere nel kernel estendendo:

- `assistant_output.rs`
- `turn_assembly.rs`

Non va creato un secondo parser parallelo in bridge o GUI.

## Regola 2. Non cambiare il modello di invocation di questa iterazione

Per tool/action il modello professionale qui e' gia' quello attuale:

- detection kernel-side
- `InvocationUpdated`
- persistenza strutturata dello stato invocation

Il fix thinking deve integrarsi con questo, non ridefinirlo.

## Regola 3. Riutilizzare la tabella `session_messages`

Per questa iterazione non serve introdurre una nuova tabella `turn_segments`.

Lo schema esistente ha gia':

- `role`
- `kind`
- `content`
- `ordinal`

Quindi il modo piu' pulito e meno invasivo e':

- `role=user kind=prompt`
- `role=assistant kind=message`
- `role=assistant kind=thinking`
- `role=system kind=invocation`
- `role=system kind=marker` o equivalente finish marker

Questo e' abbastanza espressivo per allineare live, storage e projection senza aprire una migrazione strutturale piu' larga del necessario.

## Regola 4. Il thinking e' persistibile ma non replayabile di default

Il prompt replay/resume deve continuare a materializzare solo cio' che il modello deve realmente "vedere" come cronologia conversazionale.

Quindi:

- `assistant kind=message` entra nel replay
- `assistant kind=thinking` **non** entra nel replay automatico

Questa esclusione deve essere esplicita nel design.

## Regola 5. Bridge parser thinking solo come compat layer

Finche' esistono record storici legacy o live session non migrate, `live_timeline/parser.rs` puo' restare come fallback. Pero':

- il nuovo path canonico deve preferire segmenti tipizzati dal kernel/storage
- il parser raw `<think>` non deve piu' essere la fonte primaria della semantica

## Regola 6. Separare continuity semantics da persistence semantics

Il sistema deve distinguere esplicitamente due responsabilita':

- cosa viene mostrato/persistito come storia canonica del turno
- cosa serve solo a far proseguire correttamente lo stesso turno assistant tra step locali successivi

Best practice qui:

- la storia canonica deve rimanere pulita e tipizzata (`assistant/message`, `assistant/thinking`)
- la continuita' del turno in-flight deve poter mantenere anche raw inline thinking non ancora chiuso

Se queste due semantiche vengono fuse in un solo buffer, il sistema inevitabilmente sacrifica uno dei due obiettivi:

- o sporca il replay/history con raw `<think>`
- o rompe la continuita' del thinking quando il turno e' spezzato in molti step

---

# Modello semantico approvato per questa iterazione

## Canonical turn units

- `user/prompt`
- `assistant/message`
- `assistant/thinking`
- `system/invocation` con payload strutturato e status lifecycle
- `system/marker` per finish o marker di servizio

## Importante

Per questa iterazione la verita' canonica non e':

- il raw stream
- il parser Tauri
- il contenuto `<think>` embedded nel testo assistant storico

La verita' canonica deve diventare:

- assembly kernel-side per il live
- `session_messages(role, kind, content, ordinal)` per il persisted

---

# Piano implementativo corretto

## Fase 1. Kernel output assembly

Estendere `TurnAssemblyStore` e l'accumulatore assistant per distinguere:

- visible text
- thinking text
- syscall candidate

Vincoli:

- `TOOL:` / `ACTION:` dentro un blocco thinking non devono dispatchare syscall
- marker parziali `<thi`, `nk>` e `</think>` su chunk diversi devono essere gestiti correttamente
- visible text e thinking devono poter alternarsi nello stesso turno

## Fase 2. Backend/provider reasoning sidecar

Far evolvere il path backend in modo che `reasoning_content` non venga:

- ignorato
- convertito in visible assistant text

ma trasportato come segmento `assistant/thinking`.

Questo puo' richiedere l'evoluzione delle strutture di decode/stream del backend, non solo una patch del bridge.

## Fase 2-bis. Continuazione corretta dei turni assistant locali multi-step

Per i backend locali resident con `chunk_tokens` bassi, introdurre una doppia rappresentazione dell'output assistant in-flight:

- `complete_assistant_text`: testo visibile canonico da consolidare nel prompt/storia conversazionale
- `continuation_text`: raw continuation buffer del turno assistant corrente, inclusivo di eventuale `<think>` non ancora chiuso

Requisiti:

- `token_path.rs` non deve usare solo `complete_assistant_text` per la prosecuzione di step non terminali
- il processo deve avere un suffix effimero di continuation separato dal prompt canonico persistibile
- alla chiusura vera del turno il suffix effimero deve essere svuotato
- il resume/replay storico deve continuare a ignorare `assistant/thinking`

Importante:

- questa fase e' richiesta per rendere corretto il sistema con scheduling granulare
- non e' un'ottimizzazione
- non dipende dal supporto append-only del backend
- l'eventuale transport append-only e' solo una possibile evoluzione successiva

## Fase 3. Persistenza

Quando il kernel chiude un segmento:

- persist `assistant/message` per visible text
- persist `assistant/thinking` per reasoning
- persist `system/invocation` per lifecycle tool/action

La persistenza deve avvenire ai boundary semantici, non per chunk arbitrario.

## Fase 4. Resume/replay

Aggiornare `render_prompt_from_replay_history(...)` affinche':

- includa `user`
- includa `assistant kind=message`
- escluda `assistant kind=thinking`
- continui a ignorare i record system non conversazionali

Questo e' obbligatorio.

## Fase 5. Bridge e history projection

Aggiornare i lettori Tauri per preferire i segmenti tipizzati:

- `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`
- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/*`

Il parser raw `<think>` deve sopravvivere solo come fallback legacy.

## Fase 6. GUI

La GUI deve continuare a fare solo rendering:

- `assistant_message` per visible text
- `thinking` per reasoning
- tool/action box da invocation strutturate

Il componente UI e' gia' vicino al target; va cambiata la fonte dei dati, non il principio del rendering.

---

# Test richiesti davvero

## Kernel / assembly

- parsing corretto di `<think>...</think>`
- marker parziali su chunk distinti
- testo visibile prima, durante e dopo thinking
- tool marker dentro thinking che **non** genera dispatch
- tool marker fuori thinking che continua a generare dispatch

## Backend / reasoning sidecar

- `reasoning_content` viene instradato a `assistant/thinking`
- `reasoning_content` non finisce nel visible text

## Persistenza

- un turno con visible + thinking + invocation persiste righe con `kind` corretti
- nessun thinking viene serializzato come `assistant/message`

## Resume

- il replay ricostruisce il prompt senza `assistant/thinking`
- la timeline storica continua comunque a mostrare il thinking
- la continuation effimera di un turno in-flight non viene mai persistita/replayata come history

## Local multi-step continuity

- con `chunk_tokens` bassi un blocco `<think>` aperto non deve causare ripartenze dal prefisso al passo successivo
- il raw continuation buffer del turno deve restare coerente fino alla chiusura del thinking
- il visible text consolidato deve restare pulito e separato dal thinking

## Bridge / history

- live timeline preferisce segmenti tipizzati
- history timeline preferisce record tipizzati
- il fallback parser raw serve solo per dati legacy

## GUI

- il thinking appare nel box dedicato
- il visible text non ingloba il thinking
- il box tool continua a dipendere da invocation strutturate, non dal raw text

---

# Criterio finale di accettazione

L'iterazione si puo' considerare chiusa solo quando sono vere tutte queste condizioni:

1. Il kernel e' l'unico owner della semantica del thinking.
2. Il bridge non decide piu' il thinking come path primario.
3. La history non ha bisogno di riparsare raw assistant text per mostrare il thinking nei nuovi turni.
4. Il resume non reinietta il thinking nel prompt del modello.
5. Tool/action restano modellati con l'event model strutturato gia' esistente.
6. I backend locali resident restano corretti anche con `chunk_tokens` bassi e blocchi `<think>` che attraversano piu' step.

---

# Conclusione

La proposta iniziale era **direzionalmente giusta ma architetturalmente incompleta**.

La scelta piu' professionale per questo repository non e':

- rifare da zero il modello del turno
- aggiungere un nuovo layer semantico parallelo
- spostare solo il parser `<think>` lasciando irrisolto il replay

La scelta corretta e':

- estendere la turn assembly kernel-side gia' presente
- riutilizzare la persistenza tipizzata esistente basata su `role/kind`
- trattare il thinking come segmento semantico persistibile ma non replayabile
- introdurre una continuation effimera separata per i turni assistant locali multi-step
- lasciare il parser bridge-side solo come compatibilita' legacy temporanea

Solo dopo questo riallineamento il fix thinking e' coerente con la direzione architetturale del progetto.
