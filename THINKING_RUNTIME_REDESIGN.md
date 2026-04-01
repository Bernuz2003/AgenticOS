# Thinking Runtime Redesign

## Obiettivo

Rendere la gestione del reasoning assistant generalizzata, corretta e professionale separando in modo esplicito:

- stato di esecuzione in-flight del turno assistant
- proiezione canonica per prompt/history/resume
- proiezione UI/timeline live

L'obiettivo non e' solo risolvere il bug del looping con `chunk_tokens` bassi, ma eliminare la causa architetturale: oggi il sistema usa ancora rappresentazioni parzialmente sovrapposte per scopi diversi.

## Stato Attuale

Oggi il sistema e' migliorato rispetto alla versione legacy, ma resta strutturato cosi':

- il kernel segmenta `message` e `thinking` per timeline e storage
- il processo mantiene un prompt canonico testuale (`rendered_prompt_cache`)
- durante i turni assistant multi-step esiste uno stato effimero aggiunto in modo incrementale per preservare la continuation locale
- il replay/resume usa solo il transcript canonico e ignora `assistant/thinking`

Questa soluzione e' corretta come fix incrementale, ma lascia ancora una distinzione implicita invece che modellata esplicitamente.

## Limiti Dello Stato Attuale

- Il reasoning inline e il visible text sono ancora derivati da un parser streaming che produce sia segmenti persistibili sia stato di continuation.
- Lo stato in-flight del turno assistant non e' un oggetto di dominio autonomo: e' distribuito tra `TurnAssemblyStore` e `AgentProcess`.
- Il prompt di inferenza e il prompt canonico restano concettualmente vicini ma semanticamente diversi.
- I boundary del turno assistant, del syscall, del completion stop e del replay non sono rappresentati come transizioni esplicite di una macchina a stati dedicata.
- L'integrazione tra reasoning inline e reasoning sidecar (`reasoning_content`) e' corretta, ma ancora asimmetrica: il sidecar e' proiettato, non veramente modellato come parte di uno stesso stato assistant in-flight.

## Redesign Proposto

Introdurre un oggetto runtime esplicito, ad esempio `InFlightAssistantTurn`, posseduto dal kernel e indicizzato per `pid`.

### Struttura Proposta

`InFlightAssistantTurn` dovrebbe contenere almeno:

- `raw_transport_text`
- `visible_projection`
- `thinking_projection`
- `reasoning_sidecar_projection`
- `pending_invocation`
- `pending_visible_buffer`
- `pending_thinking_buffer`
- `in_thinking_block`
- `generated_token_count`
- `phase`

Dove `phase` rappresenta in modo esplicito stati come:

- `StreamingVisible`
- `StreamingThinking`
- `InvocationPending`
- `AwaitingBoundaryCommit`
- `Closed`

## Cosa Cambierebbe Rispetto Ad Oggi

### 1. Il processo non conserverebbe piu' il raw assistant in-flight

Oggi `AgentProcess` ha ancora uno stato effimero di continuation. Nel redesign completo:

- `AgentProcess` conserva solo stato canonico di contesto
- `InFlightAssistantTurn` conserva tutto il raw non ancora consolidato
- il worker di inferenza chiede al kernel un `RenderedInferencePrompt` derivato da:
  - prompt canonico del processo
  - snapshot dell'`InFlightAssistantTurn`

Questo separa nettamente stato conversazionale stabile e stato transiente di generazione.

### 2. Il parser smette di essere il centro semantico

Oggi parsing e segmentazione coincidono quasi completamente. Nel redesign:

- il parsing produce eventi semantici assistant
- un reducer di stato aggiorna `InFlightAssistantTurn`
- storage, UI e replay leggono proiezioni diverse dello stesso stato

Quindi non avremmo piu' un unico accumulatore che implicitamente governa tutto.

### 3. I boundary diventano transizioni esplicite

Boundary come:

- model stop
- stop marker
- tool invocation
- syscall completion
- errore
- kill
- resume

devono diventare transizioni formali che producono effetti chiari:

- commit della proiezione canonica
- flush timeline/storage
- reset dello stato in-flight
- eventuale apertura di un nuovo `InFlightAssistantTurn`

### 4. Reasoning inline e sidecar convergono nello stesso modello

Nel redesign:

- `<think>...</think>` e `reasoning_content` sono due sorgenti diverse
- ma alimentano la stessa projection semantica `thinking`
- la continuation usa il raw necessario al backend specifico
- storage e replay restano backend-agnostic

Questo rende il sistema piu' generale rispetto ai modelli che emettono reasoning in modi diversi.

## Architettura Target

### Livello 1: Transport Normalization

Responsabilita':

- delta vs cumulative chunk normalization
- stop conditions di transport
- estrazione raw `text` e raw `reasoning`

Output:

- `AssistantTransportEvent`

### Livello 2: Assistant Turn Reducer

Responsabilita':

- aggiornare `InFlightAssistantTurn`
- mantenere le proiezioni `visible`, `thinking`, `invocation`
- calcolare il `continuation prompt slice`

Output:

- `AssistantTurnDelta`
- `AssistantTurnSnapshot`

### Livello 3: Projection Sinks

Responsabilita':

- timeline live
- storage canonico
- prompt inference
- replay/resume

Ogni sink legge la proiezione giusta senza reinterpretare il raw.

## Benefici

- `chunk_tokens` basso senza loop o restart semantici del thinking
- maggiore compatibilita' con backend diversi
- meno coupling tra UI, storage, prompt continuation e replay
- boundary piu' prevedibili e testabili
- resume piu' sicuro perche' basato solo su stato canonico
- maggiore facilita' nel supportare futuri modelli con canali reasoning separati

## Piano Di Migrazione

### Fase 1

Estrarre l'attuale stato effimero in una struttura dedicata mantenendo invariati storage e protocollo eventi.

### Fase 2

Far dipendere `workers/inference.rs` da un `RenderedInferencePrompt` prodotto dal reducer assistant invece che da stringhe composte direttamente da `AgentProcess`.

### Fase 3

Unificare reasoning inline e sidecar nello stesso reducer assistant.

### Fase 4

Ridurre `TurnAssemblyStore` a projection store puro o sostituirlo con il nuovo `InFlightAssistantTurnStore`.

### Fase 5

Documentare invarianti e contratti:

- cosa entra nel prompt canonico
- cosa resta solo runtime
- cosa viene persistito
- cosa e' replayabile

## Invarianti Da Rendere Esplicite

- Il transcript canonico non deve mai dipendere dal raw transport.
- Il reasoning persistito non deve mai essere replayato come assistant visible.
- La continuation locale deve poter usare stato effimero non persistibile.
- Nessun layer UI o storage deve dover riparsare il raw assistant per capire la semantica del thinking.

## Valutazione

Rispetto allo stato attuale, questa riprogettazione e' piu' pulita e professionale perche' rende esplicita una separazione che oggi esiste gia' nei fatti ma non ancora nel modello di dominio.

Il fix incrementale appena implementato e' corretto e pragmatico.
Il redesign proposto e' l'end-state architetturale consigliato.
