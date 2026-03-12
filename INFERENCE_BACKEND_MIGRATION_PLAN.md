# AgenticOS — Inference Backend Migration Plan

> Versione: `draft-2` · Data: `2026-03-12`

Questo documento definisce la migrazione strategica del motore di inferenza di AgenticOS:

- rimozione completa del backend nativo Candle e di tutto il codice che lo supporta;
- adozione di `llama.cpp` come unico motore di inferenza locale;
- preparazione esplicita del kernel al supporto futuro di backend cloud/API.

Il documento e' operativo: descrive decisioni, obiettivi, architettura target, workstream, fasi di implementazione, rischi e criteri di accettazione.

---

## 1. Decisione architetturale

### Decisione approvata

AgenticOS migra da un modello "kernel owns the inference engine" a un modello "kernel owns the agent runtime, while inference runs on pluggable backends".

In pratica:

- Candle viene rimosso completamente dal prodotto e dal kernel.
- `llama.cpp` locale diventa l'unico motore di inferenza per agenti locali persistenti.
- I backend cloud non vengono trattati come equivalente perfetto dei backend residenti locali.
- Il kernel continua a possedere scheduling, process lifecycle, tool runtime, audit, context policy e orchestration.

### Posizione di progetto

Non adottiamo una migrazione "tutto HTTP, tutto stateless".

Adottiamo invece una piattaforma capability-aware con due classi runtime attive:

1. `resident_local`:
   backend locale con stato residente e KV/cache riusabile, target principale.
2. `remote_stateless`:
   backend API/cloud, utile per burst workload o compatibilita' modello, ma con semantica diversa.

---

## 2. Perche' migriamo

### Driver principali

1. Prestazioni reali osservate
- caricamento modello piu rapido su backend esterni;
- TTFT percepito piu competitivo;
- maggiore maturita' del runtime di inferenza esterno.

2. Compatibilita' modelli
- Candle limita l'adozione di architetture recenti;
- il costo di inseguire nuovi modelli a livello kernel non e' sostenibile;
- backend esterni aprono la strada a Qwen 3.5+, modelli futuri e provider cloud.

3. Evoluzione del prodotto
- AgenticOS deve restare un OS per agenti, non un fork permanente di un inference engine;
- il valore differenziante sta nel runtime agentico, non nel forward pass.

### Cosa non vogliamo perdere

1. Residenza degli agenti
2. Cambio di contesto efficiente
3. Tool calling con round-trip minimo
4. OOM prevention e admission control
5. Identita' local-first del kernel

---

## 3. Stato attuale del kernel

### Punti gia pronti

- Il kernel e' gia backend-aware.
- Esiste una separazione tra process model e backend model.
- `external-llamacpp` esiste gia come backend integrato.
- Il process model possiede `context_slot_id`.
- Il trait `ContextSlotPersistence` e' gia presente.
- Il backend `external-llamacpp` implementa gia:
  - `id_slot`
  - `cache_prompt`
  - `save`
  - `restore`
  - `erase`

### Gap reali oggi

1. `NeuralMemory` e lo swap fisico portano ancora debito strutturale ereditato da Candle.
2. Lo strato di persistenza slot lato backend e' incompleto per backend esterni.
3. Il protocollo di generazione e' ancora modellato come step sincroni con round-trip HTTP frequenti.
4. Mancano capability esplicite per distinguere backend residenti da backend stateless.
5. Mancano politiche runtime differenziate per local resident vs cloud.

---

## 4. Principi guida della migrazione

### Principio 1

Il kernel continua a possedere il control plane.

Questo include:

- process lifecycle;
- scheduler;
- syscall/tool runtime;
- orchestration;
- audit;
- context policy;
- policy di residenza.

### Principio 2

Il kernel non deve possedere necessariamente la KV cache fisica.

Deve pero possedere:

- il mapping PID -> backend session/slot;
- la policy di allocazione/residenza;
- le decisioni di swap/eviction ad alto livello;
- la telemetria e gli SLA runtime.

### Principio 3

Le capability del backend governano il comportamento del kernel.

Non tutte le feature sono disponibili su tutti i backend.

### Principio 4

I backend cloud non ricevono la stessa promessa semantica dei backend residenti locali.

Per i cloud backend non promettiamo:

- resume al token successivo;
- KV residency;
- swap fisico;
- latenza costante sui tool turn.

### Principio 5

Il protocollo tool e context management deve restare backend-agnostic.

---

## 5. Architettura target

## 5.1 Control plane vs execution plane

### Control plane AgenticOS

Responsabile di:

- processi e PID;
- priorita' e quote;
- tool dispatch;
- contesto logico;
- orchestration DAG;
- GUI telemetry;
- audit e policy.

### Execution plane

Responsabile di:

- prompt processing;
- generazione token;
- gestione KV/cache backend-specific;
- persistenza backend-specific del contesto fisico;
- streaming token.

---

## 5.2 Capability model

Va introdotto un modello di capability esplicito, ad esempio:

```rust
pub struct BackendCapabilities {
    pub resident_kv: bool,
    pub persistent_slots: bool,
    pub save_restore_slots: bool,
    pub prompt_cache_reuse: bool,
    pub streaming_generation: bool,
    pub structured_output: bool,
    pub cancel_generation: bool,
    pub memory_telemetry: bool,
    pub tool_pause_resume: bool,
    pub context_compaction_reset: bool,
    pub parallel_sessions: bool,
}
```

### Uso delle capability

- Lo scheduler usa `resident_kv`, `parallel_sessions` e `memory_telemetry`.
- Il process runtime usa `persistent_slots`, `save_restore_slots` e `cancel_generation`.
- Il tool runtime usa `tool_pause_resume` e `streaming_generation`.
- La GUI espone le differenze semantiche per backend.

---

## 5.3 Classi di backend

### A. Resident local backend

Esempi:

- `llama.cpp` via `llama-server`

Ruolo:

- default target per agenti residenti;
- multi-agente locale;
- slot residency;
- save/restore slot;
- tool loops a basso costo relativo.

### B. Remote stateless backend

Esempi:

- OpenAI-style APIs;
- provider cloud;
- self-hosted HTTP stateless.

Ruolo:

- compatibilita' modelli;
- burst compute;
- task non residenti;
- orchestration non-latency-critical.

---

## 6. Valutazione tecnica delle tre criticita'

## 6.1 Memoria e OOM prevention

### Rischio reale

Con un backend esterno non controlliamo direttamente la RAM della KV cache.

### Valutazione oggettiva

Questo e' un peggioramento reale se il confronto e' con uno swap byte-level completamente interno.

Pero non rende il sistema ingestibile.

### Strategia target

Spostare il modello mentale da:

- "kernel paging fisico della KV"

a:

- "kernel residency manager + backend slot manager + admission control".

### Mitigazioni con `llama.cpp`

- slot espliciti;
- `id_slot`;
- `cache_prompt`;
- parallelismo configurabile;
- save/restore slot;
- erase slot;
- contesto totale governato dal server.

### Conseguenza progettuale

Per backend residenti locali, `NeuralMemory` non resta un pager universale.
Diventa principalmente un `residency_manager` per processi, slot e admission control.

Il vecchio paging fisico nato per Candle non resta nel target architetturale finale:

1. viene isolato per consentire la migrazione;
2. viene poi rimosso insieme ai path di persistenza raw-bytes che servivano solo Candle.

### Outcome desiderato

Il kernel previene OOM non tramite swap fisico universale, ma tramite:

- budget per slot;
- budget per sessione;
- coda di admission;
- politiche di eviction;
- park/resume di processi;
- spill to disk del contesto logico e, se supportato, dello slot backend.

---

## 6.2 Context switching e prompt reprocessing

### Rischio reale

Con un backend stateless puro, ogni resume richiede replay del prompt.

### Valutazione oggettiva

Per cloud/stateless il problema resta.
Per `llama.cpp` locale residente, il problema e' fortemente mitigabile.

### Strategia target

Per `resident_local`:

- ogni PID ha uno slot stabile o rilocabile;
- il kernel mantiene il mapping PID -> slot;
- il suffix prompt viene aggiunto con reuse del prefisso;
- compaction e retrieval restano logiche del kernel.

Per `remote_stateless`:

- non promettiamo resume istantaneo;
- usiamo `ContextPolicy` per compattazione e retrieval;
- i processi remoti sono trattati come "sessioni ricostruibili", non come "sessioni residenti".

### Implicazione chiave

Le API cloud non devono imporre il semplificare al ribasso tutta l'architettura locale.

---

## 6.3 Syscall e tool calling

### Rischio reale

Un backend esterno aumenta il costo del tool loop.

### Valutazione oggettiva

Il peggioramento non e' binario.

Esistono tre livelli:

1. in-process pause/resume
   migliore possibile.
2. resident local slot reuse
   costo moderato e accettabile.
3. remote stateless replay
   costo alto.

### Strategia target

Per `resident_local`:

- generazione streaming o chunked;
- intercettazione syscall al primo marker;
- stop generazione appena possibile;
- tool execution nel kernel;
- reiniezione del solo suffix contestuale;
- reuse del contesto gia presente nello slot.

Per `remote_stateless`:

- ridurre il numero di tool turn;
- preferire structured tool plans;
- aggregare tool result in una sola continuation quando possibile.

### Conseguenza

Il protocollo tool non cambia concettualmente, ma la policy di esecuzione diventa backend-specific.

---

## 7. Decisione su `llama.cpp`

### Ruolo assegnato

`llama.cpp` diventa il backend primario per:

- agenti locali residenti;
- multi-sessione locale;
- supporto ai modelli open-source moderni;
- semantica quasi-resident del contesto.

### Cosa sfruttiamo esplicitamente

- `id_slot`
- `cache_prompt`
- slot save/restore/erase
- parallel slots
- continuous batching / multi-user scheduling lato server
- structured output JSON/schema dove utile

### Cosa non assumiamo ciecamente

- telemetria memoria completa;
- equivalenza perfetta al pause/resume in-process;
- scheduling fairness perfetto lato server;
- supporto cloud-style function calling uniforme.

---

## 8. Refactor target del kernel

## 8.1 Backend abstraction v2

### Obiettivo

Separare definitivamente:

- driver selection;
- execution backend;
- residency semantics;
- persistence semantics.

### Azioni

1. Introdurre `BackendClass`.
2. Introdurre `BackendCapabilities`.
3. Espandere `DriverResolution` con classe e capability.
4. Rendere `RuntimeModel` capability-aware.
5. Rendere `STATUS` e backend diagnostics capaci di esporre tali capability.

### File principali

- `crates/agentic-kernel/src/backend/mod.rs`
- `crates/agentic-kernel/src/backend/diagnostics.rs`
- `crates/agentic-kernel/src/commands/model.rs`
- `crates/agentic-kernel/src/commands/status.rs`

---

## 8.2 Refactor di `NeuralMemory`

### Obiettivo

Trasformare `NeuralMemory` da componente implicitamente Candle-centric a subsystem centrato sulla residency locale e sui backend residenti.

### Direzione

Scindere in:

1. `LogicalResidencyManager`
- PID residency state
- slot assignment
- eviction policy
- waiting queues
- parked/resident/swapped state

2. `BackendSlotPersistence`
- save/restore/erase per backend residenti esterni

3. rimozione del vecchio `PhysicalPagedMemory`
- dopo il completo distacco dai backend Candle
- con contestuale rimozione dei path `physical_payload`

### Azioni

1. Isolare il concetto di "PID waiting for memory" da "OOM in pager fisico".
2. Rimuovere l'assunzione che `write_for_pid_bytes_with_backend()` sia il flusso universale.
3. Modellare eventi distinti:
   - `resident_slot_saved`
   - `resident_slot_restored`
   - `resident_slot_evicted`
   - `backend_memory_pressure`
4. Eliminare a regime i code path e i tipi che esistono solo per supportare lo swap fisico Candle.

### File principali

- `crates/agentic-kernel/src/memory/core.rs`
- `crates/agentic-kernel/src/memory/swap.rs`
- `crates/agentic-kernel/src/memory/types.rs`
- `crates/agentic-kernel/src/runtime/mod.rs`

---

## 8.3 Process model v2

### Obiettivo

Rendere il processo indipendente dal fatto che il backend sia locale residente o remoto.

### Azioni

1. Aggiungere uno stato di residency backend-specific.
2. Rendere esplicito il binding:
   - PID -> backend session id
   - PID -> context slot id
   - PID -> backend class
3. Separare:
   - contesto logico del kernel
   - contesto fisico del backend

### File principali

- `crates/agentic-kernel/src/process.rs`
- `crates/agentic-kernel/src/engine/lifecycle.rs`
- `crates/agentic-kernel/src/services/process_runtime.rs`

---

## 8.4 Tool runtime v2

### Obiettivo

Ridurre il costo del tool loop sui backend esterni residenti.

### Azioni

1. Passare da generazione "step by step bloccante" a "stream controllato fino a marker o stop".
2. Abilitare cancellazione/interruzione di generazione quando il marker tool viene rilevato.
3. Reiniettare solo il suffix tool result, non il prompt completo.
4. Tenere il parsing tool separato dal formato di trasporto.

### File principali

- `crates/agentic-kernel/src/runtime/inference_results.rs`
- `crates/agentic-kernel/src/runtime/syscalls.rs`
- `crates/agentic-kernel/src/backend/external_llamacpp.rs`
- `apps/agent-workspace/src-tauri/src/kernel/stream.rs`

---

## 8.5 Cloud backend readiness

### Obiettivo

Preparare il kernel a backend cloud senza degradare l'esperienza locale.

### Azioni

1. Introdurre un backend class `remote_stateless`.
2. Definire politiche runtime diverse:
   - max tool turns piu basso
   - context compaction piu aggressiva
   - no promise di slot persistence
3. Introdurre provider config sicura:
   - endpoint
   - auth
   - timeouts
   - rate limits
   - cost telemetry
4. Rendere l'orchestrator backend-aware.

### File principali

- `crates/agentic-kernel/src/backend/mod.rs`
- `crates/agentic-kernel/src/config.rs`
- `crates/agentic-kernel/src/orchestrator/mod.rs`
- `crates/agentic-kernel/src/policy/mod.rs`

---

## 9. Piano operativo dettagliato

## Fase 0 — Formalizzazione e guardrail

**Obiettivo**
- congelare il perimetro della migrazione.

**Task**
1. Approvare questo documento come fonte di verita.
2. Formalizzare `resident_local` e `remote_stateless` come uniche classi runtime di prodotto.
3. Definire capability minime richieste per ogni backend.
4. Definire i contratti semantici supportati per classe di backend.

**Deliverable**
- documento approvato;
- capability matrix;
- backlog rifinito per workstream.

**DoD**
- nessun modulo nuovo implementato senza riferimento a capability/semantics esplicite.

**Tracking implementativo**
- [x] Documento di migrazione creato e usato come fonte di verita.
- [x] Classi runtime attive `resident_local` e `remote_stateless` formalizzate; `embedded` rimossa dal codebase runtime.
- [x] Capability model unificato definito e propagato nei payload condivisi.
- [ ] Backlog operativo derivato per milestone/file ownership.

---

## Fase 1 — Backend abstraction v2

**Obiettivo**
- rendere il kernel capability-aware.

**Task**
1. Introdurre `BackendClass`.
2. Introdurre `BackendCapabilities`.
3. Estendere `DriverDescriptor` e `DriverResolution`.
4. Esporre le capability in diagnostica.
5. Aggiornare `LIST_MODELS`, `MODEL_INFO`, `STATUS`.

**Deliverable**
- abstraction layer v2;
- diagnosi backend leggibile da GUI e test.

**DoD**
- ogni backend dichiara classe e capability;
- GUI/Tauri puo mostrare se un backend supporta slot residenti o no.

**Tracking implementativo**
- [x] `BackendClass` introdotta nel kernel.
- [x] `BackendCapabilities` introdotta nel kernel.
- [x] `DriverDescriptor` e `DriverResolution` estesi con classe e capability.
- [x] `RuntimeModel` reso capability-aware.
- [x] `LIST_MODELS`, `MODEL_INFO`, `LOAD` aggiornati con classe e capability.
- [x] `STATUS` e backend diagnostics aggiornati con capability e metadata backend.
- [x] GUI/Tauri aggiornata per mostrare backend class e supporto slot residenti.

---

## Fase 2 — Hardening `llama.cpp` come backend residente

**Obiettivo**
- fare di `llama.cpp` il backend locale primario, non un adapter sperimentale.

**Task**
1. Stabilizzare timeout, streaming e gestione errori.
2. Introdurre un vero slot manager lato kernel.
3. Tenere mapping PID -> slot persistente e osservabile.
4. Integrare save/restore/erase come flusso ufficiale.
5. Migliorare diagnostica server:
   - health
   - props
   - slots
   - limits

**Deliverable**
- backend `external-llamacpp` production-grade;
- lifecycle slot completo.

**DoD**
- un PID puo essere:
  - avviato;
  - parcheggiato;
  - restaurato;
  - liberato;
  senza perdere coerenza di stato.

**Tracking implementativo**
- [x] Timeout backend esterno resi configurabili e messaggi di errore migliorati.
- [x] Diagnostica `health`, `props` e `slots` esposta per `external-llamacpp`.
- [x] Mapping `PID -> context_slot_id` reso osservabile in kernel, Tauri e GUI.
- [x] Persistenza slot distinta tra `physical_payload` e `backend_slot_snapshot`.
- [x] Flusso ufficiale `save/restore/erase` integrato per `external-llamacpp`.
- [x] Helper PID-based `save/load/free` introdotti nel lifecycle engine.
- [x] Stato locale del resident slot introdotto sul `process model`.
- [x] Surface pubblica `STATUS`/GUI riallineata a `Parked` e `parked_pids`.
- [x] Slot manager kernel-side completo con policy di `park/resume` esplicita.
- [x] Stato `parked/resident/free` separato definitivamente dal vecchio `WaitingForMemory`.

---

## Fase 3 — Refactor `NeuralMemory` in residency manager

**Obiettivo**
- scollegare la gestione runtime del processo dalla sola memoria fisica interna.

**Task**
1. Estrarre `LogicalResidencyManager`.
2. Spostare in esso:
   - admission control
   - parking
   - resident/waiting state
   - slot ownership
3. Rendere `PhysicalPagedMemory` uno strato transitorio e poi rimuoverlo.
4. Rendere gli eventi di swap backend-neutral.
5. Rimuovere il save/restore fisico raw-bytes che serviva solo Candle.

**Deliverable**
- memory subsystem centrato sulla residency locale.

**DoD**
- il kernel gestisce i backend `resident_local` senza dipendere dal pager fisico Candle.

**Tracking implementativo**
- [x] Eventi di swap resi backend-neutral tramite `SlotPersistenceKind`.
- [x] Restore path separato tra payload fisico transitorio e snapshot backend-owned.
- [x] `LogicalResidencyManager` estratto da `NeuralMemory`.
- [x] `PhysicalPagedMemory` isolato come componente transitorio.
- [x] Admission control, parking e slot ownership spostati nel nuovo manager.
- [x] `NeuralMemory` ridotto a dual-mode subsystem esplicito.
- [x] `PhysicalPagedMemory`, `physical_payload` e i code path Candle-only rimossi dal codebase.

---

## Fase 4 — Tool/runtime streaming model

**Obiettivo**
- ridurre round-trip e TTFT operativo nei loop tool.

**Task**
1. Evolvere l'adapter esterno da `stream: false` rigido a modalita' stream-aware.
2. Introdurre stop anticipato sul marker tool.
3. Ridurre la continuation post-tool al solo suffix contestuale.
4. Hardening del parser tool in streaming.
5. Migliorare timeline e audit lato GUI.

**Deliverable**
- tool loop competitivo su backend esterno locale.

**DoD**
- i turn con tool non richiedono replay integrale del prompt quando lo slot e' residente.

**Tracking implementativo**
- [x] Parser tool aggiornato per supportare sia sintassi legacy sia `TOOL:` canonico.
- [x] Adapter `external-llamacpp` reso stream-aware.
- [x] Stop anticipato sul marker tool in modalita' streaming.
- [x] `AgentProcess` mantiene un `rendered_prompt_cache` append-only per evitare `tokenizer.decode(tokens)` completo nei tool turn residenti.
- [x] Reinjection post-tool ridotta al solo suffix contestuale tramite checkpoint del prompt residente e tracking del suffix post-syscall.
- [x] Strategia transport esplicita introdotta: `llama.cpp` usa fallback `full prompt + cache_prompt` con prefisso comune; l'append-only puro resta un vincolo upstream documentato, non un gap del kernel.
- [x] Timeline e audit GUI aggiornati per il lifecycle streaming dei tool turn.

---

## Fase 5 — Candle removal

**Obiettivo**
- rimuovere completamente Candle dal kernel, dal catalogo driver e dai path runtime.

**Task**
1. Rimuovere Candle dal percorso di default.
2. Rimuovere driver, loader, config, benchmark e test che esistono solo per Candle.
3. Eliminare `PhysicalPagedMemory` e i path `physical_payload` usati per il suo supporto.
4. Aggiornare routing, documentazione e benchmark sul nuovo perimetro.

**Deliverable**
- `llama.cpp` default local backend;
- Candle assente dal codebase.

**DoD**
- il prodotto compila, testa e documenta un solo motore locale: `llama.cpp`.

**Tracking implementativo**
- [x] Routing locale di default spostato su `llama.cpp`.
- [x] Driver, loader e riferimenti Candle rimossi dal kernel.
- [x] `PhysicalPagedMemory` e `physical_payload` rimossi insieme ai path Candle-only.
- [x] Benchmark, documentazione e configurazione aggiornati al nuovo runtime unico locale.

---

## Fase 6 — Cloud backend onboarding

**Obiettivo**
- aggiungere backend API senza rompere la semantica locale.

**Task**
1. Introdurre un primo backend `remote_stateless`.
2. Aggiungere configurazione provider.
3. Aggiungere cost/rate telemetry.
4. Aggiungere policy runtime differenziate.
5. Rendere l'orchestrator capace di scegliere backend per task.

**Deliverable**
- supporto cloud di prima classe ma semanticamente distinto.

**DoD**
- un task puo essere routato su cloud senza che il kernel prometta false equivalenze con `resident_local`.

**Tracking implementativo**
- [x] Classe `remote_stateless` formalizzata nel modello backend.
- [ ] Primo backend cloud/API implementato.
- [ ] Config provider, auth, rate limit e cost telemetry introdotte.
- [ ] Policy runtime differenziate applicate ai backend cloud.
- [ ] Orchestrator reso backend-aware per il routing multi-provider.

---

## 10. Workstream paralleli

### Workstream A — Core backend model

- `backend/mod.rs`
- `backend/diagnostics.rs`
- `model_catalog/*`
- `commands/model.rs`

### Workstream B — Residency e memory

- `memory/*`
- `runtime/mod.rs`
- `services/process_runtime.rs`

### Workstream C — `llama.cpp` production backend

- `backend/external_llamacpp.rs`
- `backend/http.rs`
- `backend/remote_adapter.rs`
- config/diagnostics

### Workstream D — Tool runtime

- `runtime/syscalls.rs`
- `runtime/inference_results.rs`
- `tools/*`

### Workstream E — GUI e osservabilita'

- `apps/agent-workspace/src-tauri/src/kernel/*`
- `apps/agent-workspace/src/pages/*`
- `apps/agent-workspace/src/components/*`

### Workstream F — Policy e orchestration

- `policy/mod.rs`
- `orchestrator/*`
- `scheduler.rs`

---

## 11. Rischi principali

### R1. Regressione nella semantica multi-agente

**Rischio**
- il server esterno diventa un collo di bottiglia condiviso.

**Mitigazione**
- slot budgeting;
- admission control;
- benchmark multi-sessione;
- separazione tra active e parked agents.

### R2. Illusione di equivalenza tra local resident e cloud

**Rischio**
- UX e runtime promettono troppo sui backend cloud.

**Mitigazione**
- capability esplicite;
- policy differenziate;
- messaging chiaro in GUI/API.

### R3. Regressioni tool loop

**Rischio**
- troppi round-trip HTTP;
- TTFT percepito peggiore.

**Mitigazione**
- streaming controllato;
- stop anticipato;
- suffix-only reinjection;
- benchmark specifici tool-heavy.

### R4. `llama.cpp` come nuova black box non osservabile

**Rischio**
- kernel perde visibilita' runtime.

**Mitigazione**
- diagnostics forti;
- slot introspection;
- telemetry estesa;
- health checks per backend.

### R5. Debito di transizione

**Rischio**
- la rimozione incompleta del vecchio path Candle lascia codice morto e semantiche ambigue nel kernel.

**Mitigazione**
- capability model unico;
- fasi chiare;
- rimozione esplicita dei moduli legacy e dei benchmark ormai fuori perimetro.

---

## 12. Benchmark e validazione richiesti

### Benchmark funzionali

1. start/stop di un singolo PID
2. resume su slot residente
3. save/restore di slot
4. compaction + retrieval + continuation
5. tool loop con 1, 3, 10 syscall

### Benchmark prestazionali

1. model load latency
2. TTFT
3. token/sec sostenuti
4. costo medio per tool turn
5. costo medio di resume dopo park
6. throughput con N agenti residenti

### Benchmark di robustezza

1. backend unavailable
2. timeout lettura
3. slot restore failure
4. slot exhaustion
5. process eviction under pressure

---

## 13. Criteri di accettazione finali

La migrazione puo dirsi riuscita solo se tutte le condizioni seguenti sono vere:

1. `llama.cpp` e' il backend locale di default.
2. Il kernel resta process-centric e tool-centric.
3. Gli agenti residenti locali mantengono semantica di resume forte.
4. I tool turn non richiedono replay completo del prompt su backend residente.
5. Il sistema multi-agente sotto pressione degrada in modo controllato.
6. I backend cloud sono supportati senza appiattire la semantica locale.
7. Candle non esiste piu nel codebase, nella config runtime e nella documentazione operativa.

---

## 14. Non-obiettivi espliciti

Questo piano non implica:

- distributed cluster scheduling;
- sostituzione del protocollo TCP locale del kernel;
- dipendenza immediata da Tokio;
- supporto uniforme a ogni API cloud;
- equivalenza assoluta tra tutti i backend.

---

## 15. Prossima esecuzione consigliata

Ordine pragmatico:

1. Fase 1 — Backend abstraction v2
2. Fase 2 — Hardening `llama.cpp`
3. Fase 3 — Residency manager refactor
4. Fase 4 — Tool/runtime streaming model
5. Fase 5 — Candle removal
6. Fase 6 — Cloud backend onboarding

### Motivazione

Questo ordine massimizza il valore presto:

- prima rendiamo il kernel capace di ragionare sui backend;
- poi rendiamo forte il backend locale residente;
- solo dopo rifattorizziamo in profondita' memory/runtime;
- poi rimuoviamo definitivamente il debito Candle che oggi inquina memory/runtime;
- infine apriamo ai cloud senza rompere la semantica locale.

---

## 16. Sintesi finale

La direzione corretta non e' "tenere Candle come fallback permanente" ne' "uscire da Candle per entrare nel puro stateless HTTP".

La direzione corretta e':

- rimuovere Candle completamente dal prodotto;
- rendere `llama.cpp` il backend locale residente di riferimento;
- far evolvere `NeuralMemory` in un residency subsystem che non dipende piu dal pager fisico Candle;
- trattare i backend cloud come classe distinta;
- preservare il valore di AgenticOS nel control plane agentico, non nel possesso del forward pass.

Questo mantiene intatta l'identita' di AgenticOS come sistema operativo local-first per agenti, pur liberandolo dal collo di bottiglia architetturale imposto da Candle.
