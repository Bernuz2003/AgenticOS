# M47) UX/Runtime Hardening — Thinking Boxes, Professional Prompting, Runtime Stop Control, Session Configuration, Live Input Dedup

**Status:** `DONE`

## Obiettivo

Implementare una serie di fix coordinati che migliorino la qualità professionale del sistema lato:
- rendering della chat,
- affidabilità del comportamento dei modelli,
- controllo runtime durante l’inferenza,
- configurabilità delle sessioni da GUI.

Questa iterazione deve rimanere **sobria e pulita**:
- niente complessità stratificata;
- niente doppie fonti di verità;
- niente fix solo cosmetici lato GUI;
- ogni modifica deve rispettare il modello architetturale del sistema kernel → bridge → GUI.

Piano e validazione sono stati documentati in:
- `M47_IMPLEMENTATION_PLAN.md`

---

# Problema 1 — Box di thinking vuoti per modelli che emettono wrapper senza contenuto

**Status:** `DONE`

## Sintomo
Per Qwen3.5 e modelli simili può comparire il box di thinking anche quando il thinking reale è vuoto.  
Questo succede perché il modello emette i token di apertura/chiusura del thinking, ma niente contenuto effettivo tra i due.

## Stato verificato
Il problema era realmente presente nei builder bridge-side:
- `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`
- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/snapshot.rs`

Il filtro `trim().is_empty()` era già applicato ai segmenti assistant standard, ma non ai segmenti `Thinking`.

## Cosa vogliamo ottenere
- Se il thinking è semanticamente vuoto, **non deve essere renderizzato alcun box**.
- Se il thinking contiene testo reale, deve essere mostrato correttamente.
- Questo deve valere sia:
  - live
  - storico/persistito
  - resume sessione

## Esito implementato
- introdotto il filtro equivalente anche per i segmenti `Thinking`;
- applicato il filtro sia al live snapshot sia alla history projection sia al seed storico usato per il resume;
- aggiunti test per:
  - thinking vuoto → nessun box
  - thinking reale → box presente

## Nota architetturale
Questo fix non deve riportare la semantica del thinking in GUI.  
La GUI deve solo renderizzare ciò che riceve; il filtro può stare nella projection layer bridge-side dove vengono costruiti i `TimelineItem`.

---

# Problema 2 — Professionalizzazione del system prompt e prevenzione di output “finti di sistema”

**Status:** `DONE`

## Sintomo
Alcuni modelli ogni tanto:
- dichiarano che invocheranno un tool,
- poi non lo invocano davvero,
- ma generano testo che simula un messaggio di sistema, ad esempio:
  - `[system] Output: Success: Python script executed. [/system]`

Questo è un comportamento pericoloso, perché:
- produce falsi positivi lato utente;
- mina la fiducia nel sistema;
- può far sembrare completato un task che in realtà non lo è.

## Stato verificato
Il punto corretto del prompt è:
- `crates/agentic-kernel/src/prompt/agent_prompt.rs`

Il prompt era troppo minimale e mancava di guardrail espliciti su:
- differenza tra intenzione e completamento reale;
- divieto di simulare messaggi di sistema/tool result;
- uso diretto e canonico delle invocazioni;
- verifica degli esiti reali prima di dichiarare successo.

## Cosa vogliamo ottenere
Un system prompt più professionale che:
- spinga il modello a ragionare prima di agire;
- chiarisca che un task non è completato finché il kernel non ha realmente eseguito il tool e restituito un esito;
- vieti esplicitamente di simulare:
  - messaggi di sistema
  - tool result
  - successi non verificati;
- spinga a usare i tool in modo corretto, preciso e minimale.

## Però non basta il prompt
Oltre al prompt, serve valutare strategie aggiuntive lato runtime/projection per ridurre i falsi positivi.

### Strategie da valutare
- rilevare output assistant che imitano messaggi di sistema/tool result e trattarli come plain assistant text, non come eventi veri;
- eventualmente introdurre un guardrail di projection/rendering che mostri come “assistant text” qualsiasi pseudo-output non proveniente da un vero evento kernel-side;
- aggiungere test che distinguano:
  - vero invocation result
  - testo assistant che finge di essere invocation result

## Esito implementato
- revisione del system prompt con tono più professionale e istruzioni esplicite su:
  - think before act
  - distinzione tra intenzione ed esecuzione reale
  - divieto di imitare messaggi di sistema, tool result o successi non verificati
- confermato e testato l’hardening della projection: il testo assistant che imita output di sistema o tool resta plain assistant text e non viene promosso a evento reale;
- aggiunti test sui casi di pseudo-output simulato.

---

# Problema 3 — Pulsante per interrompere l’inferenza mentre il modello sta generando

**Status:** `DONE`

## Sintomo
L’utente oggi non può interrompere davvero una generazione in corso.  
Esiste già `STOP_OUTPUT`, ma oggi funziona nel caso:
- output già troncato
- stato `AwaitingTurnDecision`

Quindi non è un vero “stop while running”.

## Stato verificato
I path reali coinvolti erano:
- GUI: `apps/agent-workspace/src/components/workspace/timeline-pane/index.tsx`
- GUI page orchestration del workspace: `apps/agent-workspace/src/pages/chats/detail.tsx`
- kernel command: `crates/agentic-kernel/src/commands/process/turn_control.rs`

Le capability runtime continuano a esporre:
- `cancel_generation: false`

quindi il sistema non può promettere una cancel immediata provider-native.

## Cosa vogliamo ottenere
Un controllo utente esplicito durante la generazione:
- il pulsante compare quando il modello è davvero in inferenza;
- l’utente può chiedere di interrompere;
- il sistema reagisce in modo coerente e non ambiguo.

## Attenzione
Questo task va affrontato in modo professionale, senza fingere una cancel capability che non esiste.

### Serve distinguere due modelli possibili
#### A. Hard cancel reale
Se il backend supporta una vera cancellazione della generation.

#### B. Soft stop / stop at safe boundary
Se il backend non supporta cancel immediato, il sistema deve:
- registrare l’intenzione di stop,
- fermarsi alla prima boundary sicura possibile,
- rendere chiaro in UI lo stato.

## Esito implementato
- definita una semantica duale:
  - hard stop su `AwaitingTurnDecision`
  - soft stop request while in-flight, applicata alla prossima safe boundary
- introdotto il bottone di stop direttamente nella composer bar, a destra del campo di input, come controllo iconico stabile durante `InFlight` / `AwaitingRemoteResponse` / `Running`;
- rimossa la card flottante nel flusso della timeline che si spostava mentre il testo cresceva;
- resa la UX onesta: quando `cancel_generation` è `false`, il bottone espone il comportamento reale via hover/title e resta in stato pending dopo la richiesta di soft stop;
- implementato il flusso end-to-end coerente kernel → bridge → GUI;
- aggiunti test su:
  - richiesta di soft stop mentre il turno è in-flight
  - completamento del turno alla safe boundary successiva
  - comportamento hard stop legacy su `AwaitingTurnDecision`

## Vincolo importante
Non introdurre un bottone che “promette” cancel immediato se il runtime non può farlo davvero.  
La UX deve essere onesta e coerente con le capability reali.

---

# Problema 4 — Configurazione completa della sessione lato GUI

**Status:** `DONE`

## Sintomo
Quando si crea una nuova sessione/chat, l’utente non può configurare parametri importanti come:
- quota token
- quota syscall
- eventualmente altre policy di sessione

La sessione parte con valori impliciti e non modificabili dalla GUI.

## Stato verificato
I path reali erano:
- frontend: `apps/agent-workspace/src/pages/chats/page.tsx`
- frontend API: `apps/agent-workspace/src/lib/api/sessions.ts`
- Tauri bridge: `apps/agent-workspace/src-tauri/src/commands/sessions.rs`
- start EXEC bridge-side: `apps/agent-workspace/src-tauri/src/kernel/live_timeline/store.rs`
- kernel spawn path: `crates/agentic-kernel/src/commands/exec.rs`
- process runtime: `crates/agentic-kernel/src/services/process_runtime.rs`

La GUI non trasportava quote e il bridge usava ancora un payload `EXEC` non strutturato per questo caso.

## Cosa vogliamo ottenere
Nella creazione di una nuova sessione, l’utente deve poter decidere da GUI almeno:

### Quota token
- `No Limit`
- `Limit`
- se `Limit` → compare input numerico

### Quota syscall
- `No Limit`
- `Limit`
- se `Limit` → compare input numerico

Possibilmente con UX pulita, leggibile e coerente col resto del sistema.

## Esito implementato
### Frontend
- estesa la modal “Nuova Chat” con controlli `No Limit / Limit` per:
  - quota token
  - quota syscall
- aggiunta validazione UI dei campi numerici

### API / Tauri bridge
- esteso `startSession(...)`
- esteso `start_session(...)`
- introdotto un payload `EXEC` JSON strutturato per portare:
  - `prompt`
  - `max_tokens`
  - `max_syscalls`

### Kernel / control plane
- introdotto un override quota in fase di spawn del processo, senza dipendere da `SET_QUOTA` post-hoc;
- mantenuta una sola via coerente di inizializzazione dei limiti;
- corretto l’accoppiamento sbagliato tra quota scheduler e budget tecnico del turno:
  - `No Limit` rimuove il guardrail di sessione
  - non dilata artificialmente il budget di generation del backend

## Vincolo architetturale
La configurazione della sessione deve avere una fonte di verità chiara:
- idealmente impostata in fase di start
- non metà in GUI e metà in mutazioni runtime successive
- non con valori impliciti diversi tra frontend, bridge e kernel

---

## Problema 5 — Messaggio utente duplicato quando si invia input in una sessione già esistente

**Status:** `DONE`

### Sintomo
Quando l’utente scrive un nuovo messaggio in una sessione/chat già avviata (quindi con storico precedente), il nuovo messaggio viene visualizzato **due volte** nella timeline della chat.

### Effetto lato GUI
La chat mostra il prompt utente duplicato immediatamente dopo l’invio, generando un’esperienza visiva sporca e poco professionale.

### Root cause tecnica
Dai sorgenti attuali, la causa più plausibile e coerente è una **duplicazione tra persisted truth e live state** nel momento in cui il bridge registra il nuovo input.

In particolare:

- lato kernel, `SEND_INPUT` persiste subito il nuovo turno e il relativo messaggio utente:
  - `crates/agentic-kernel/src/commands/process/input.rs`
  - `ctx.storage.start_session_turn(...)`

- lato Tauri/bridge, dopo `send_input(...)`, viene chiamato:
  - `composer::register_live_user_input(...)`
  - `apps/agent-workspace/src-tauri/src/commands/sessions.rs`

- `register_live_user_input(...)` fa:
  1. `ensure_live_timeline_for_session_pid(...)`
  2. se non esiste già timeline live, seed della timeline dal DB storico (`load_timeline_seed(...)`)
  3. poi **appende comunque il nuovo prompt con `store.append_user_turn(...)`**

Il problema è che, se il nuovo messaggio è già stato persistito dal kernel **prima** che il bridge faccia il seed dal DB, la timeline seed può già contenere quel nuovo turno utente.  
Subito dopo, il bridge aggiunge di nuovo lo stesso prompt nel live store.

### In sintesi
Il nuovo input viene rappresentato due volte perché:
- una volta entra nella timeline tramite **persisted seed dal DB**
- una seconda volta viene aggiunto nel **live store** con append esplicita

### Perché succede soprattutto su sessioni già esistenti
Questo bug emerge tipicamente quando:
- si apre una sessione storica/persistita;
- la timeline live non è ancora stata costruita o sincronizzata;
- il primo nuovo `send_input` provoca:
  - persistenza del turno nel kernel
  - seed dal DB nel bridge
  - append live immediata dello stesso prompt

### Cosa vogliamo ottenere
Una sola fonte di verità per il nuovo input utente.

Dopo l’invio di un nuovo messaggio:
- il prompt utente deve comparire **una sola volta**;
- la timeline live e la persisted truth devono essere coerenti;
- il bridge non deve duplicare un turno già incluso nella seed dal DB.

### Come risolvere
Serve correggere la semantica di `register_live_user_input(...)` e del seeding della timeline live.

#### Direzione consigliata
Il bridge deve scegliere **una sola** di queste strategie:

##### Opzione A — Seed dal DB e niente append se il nuovo turno è già presente
Dopo il `send_input`, se il seed dal DB contiene già il nuovo turno utente, il bridge **non deve** chiamare `append_user_turn(...)`.

##### Opzione B — Append ottimistico live e niente seed del nuovo turno dal DB
Se si vuole appendere optimisticamente il prompt lato live store, allora il seed dal DB deve fermarsi allo stato precedente, senza includere il nuovo input appena persistito.

### Soluzione preferibile
La soluzione più professionale è:
- mantenere una sola fonte di verità operativa del nuovo turno;
- evitare che il bridge combini senza controllo:
  - `persisted seed`
  - `live append`
- introdurre un controllo esplicito di deduplica per il turno appena inviato, se necessario.

### Best practice architetturali
- **Una sola fonte di verità** per il nuovo turno utente.
- Il bridge deve essere un projection layer pulito, non un luogo in cui persisted e live vengono sommati ciecamente.
- La timeline live non deve duplicare contenuti già presenti nel seed storico.
- Nessuna deduplica fragile basata solo sul rendering GUI: la correzione va fatta nella pipeline di composizione dei dati, non nel componente visivo.

### File da analizzare/modificare
- `apps/agent-workspace/src-tauri/src/commands/sessions.rs`
- `apps/agent-workspace/src-tauri/src/kernel/composer/workspace.rs`
- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/store.rs`
- `apps/agent-workspace/src-tauri/src/test_support/composer.rs`
- `apps/agent-workspace/src-tauri/tests/workspace_composer.rs`

### Test richiesti
Aggiungere test che verifichino almeno questo scenario:

1. apri una sessione storica già esistente;
2. invia un nuovo input;
3. il nuovo messaggio utente compare una sola volta;
4. la timeline risultante contiene:
   - storico precedente
   - nuovo prompt
   - nessun duplicato

### Risultato finale atteso
Dopo il fix:
- il messaggio utente inviato in una sessione già esistente viene mostrato una sola volta;
- live timeline e persisted truth restano coerenti;
- la composizione della pagina chat non produce più duplicazioni.

### Esito implementato
- `register_live_user_input(...)` ora distingue tra:
  - timeline già live / rebound session → append esplicito del nuovo prompt
  - timeline appena seeded dal DB storico → nessun append se il seed contiene già il turno running appena persistito
- introdotto un controllo bridge-side sullo stato dell’ultimo turno seeded (`running`, senza messaggi assistant e con prompt uguale) per evitare doppio inserimento;
- rimossi i test inline dal file sorgente `workspace.rs` e spostati in `apps/agent-workspace/src-tauri/tests/workspace_composer.rs`, coerentemente con la direttiva di tenere i test fuori da `src/`;
- aggiunti test esterni per:
  - fallback snapshot da storico persistito
  - fallback timeline da storico persistito
  - append corretto del nuovo turno quando il seed storico non lo contiene ancora
  - assenza di duplicazione quando il seed storico contiene già il nuovo prompt running

---

# Requisiti trasversali per l’implementazione

## 1. Piano operativo obbligatorio
Completato in:
- `M47_IMPLEMENTATION_PLAN.md`

Contiene:
- root cause per ciascun punto
- file da toccare
- decisioni architetturali
- rischi/regressioni
- strategia di validazione

## 2. Nessuna complessità stratificata
Non vogliamo:
- nuove euristiche sparse
- parsing duplicato
- fonti di verità multiple
- fix solo cosmetici

## 3. Validazione seria
Completato. Sono stati aggiunti o aggiornati test per:
- thinking vuoto vs non vuoto
- output assistant che finge messaggi di sistema
- semantica e uso corretto dello stop soft/hard
- avvio sessione con quote personalizzate

## 4. UX chiara
Tutto ciò che esponi in GUI deve essere:
- coerente con le capability reali del kernel
- onesto lato utente
- pulito visivamente

---

# Risultato finale atteso

Al termine della milestone vogliamo avere:

- box di thinking mostrati solo quando il thinking contiene davvero testo;
- system prompt più professionale e modelli meno inclini a fingere esiti di sistema/tool;
- possibilità per l’utente di interrompere una generazione in corso con semantica corretta;
- configurazione completa della sessione lato GUI per quote token/syscall;
- codice più pulito e coerente, senza stratificazioni inutili.
