
## M34) Orchestrazione come feature di primo livello

**Status:** `DONE`

### Obiettivo

Trasformare l’orchestrazione da meccanismo backend già esistente a capability esplicita del prodotto.

### Cosa vogliamo ottenere

* separazione netta tra:

  * `New Chat`
  * `New Workflow`
* avvio workflow dal control-plane, non da `ACTION:spawn` dentro una chat normale
* workflow/task graph come entità di primo livello
* possibilità di definire task con:

  * ruolo
  * prompt
  * dipendenze
  * workload
  * runtime target
  * policy context
* monitoraggio chiaro di task, dipendenze e stato

### Perché è prioritario

Perché il backend ha già il motore di orchestrazione, ma non lo esprime ancora come feature vera.
Finché non emerge questo livello, AgenticOS continuerà a sembrare soprattutto una chat evoluta.

### DoD

* [x] workflow mode distinto dalla chat normale
* [x] avvio orchestrazione esplicito dal control-plane
* [x] monitor UI dei task di workflow
* [x] niente esposizione impropria di orchestrazione nella chat base

---

## M30) Process-scoped Tool Permissions & Supervisor Boundaries

**Status:** `DONE`

### Obiettivo

Introdurre una vera governabilità per processo/task su tool e action.

### Cosa vogliamo ottenere

* allowlist tool per PID / task / orchestration
* path scope e trust scope per processo
* distinzione forte tra:

  * agente interattivo normale
  * supervisore/orchestratore
  * caller programmatici / control-plane
* `ACTION:spawn/send` non disponibili ai normali agenti chat
* inheritance pulita:

  * default kernel
  * default orchestrazione
  * override task

### Perché è prioritario

Perché senza permessi per processo non puoi esporre seriamente orchestrazione, agenti pro-attivi e tool più forti.

### DoD

* [x] enforcement runtime uniforme
* [x] policy osservabile via STATUS / GUI
* [x] no orchestrazione spontanea da chat normale
* [x] audit coerente dei deny / allow

---

## M35) Artifacts & Structured Task I/O

**Status:** `DONE`

### Obiettivo

Rendere gli output dei task entità strutturate e passabili tra task/agenti.

### Cosa vogliamo ottenere

* output strutturato per task
* artifact store locale/persistito
* edges del workflow che passano artifacts, non solo messaggi liberi
* input/output task leggibili, persistiti e osservabili
* base per retry, replay parziale e debugging serio

### Perché è prioritario

Perché l’orchestrazione senza artifacts rischia di dipendere troppo da messaggi opachi tra processi.
Gli artifacts sono il vero collante di workflow robusti.

### DoD

* [x] task output strutturato persistito
* [x] artifacts visibili nella GUI
* [x] passaggio artifacts tra task supportato dal runtime
* [x] retry di task senza perdere output precedenti

---

## M36) Scheduler di sistema & Proactive Agents

**Status:** `DONE`

### Obiettivo

Introdurre job persistiti e agenti/workflow che si attivano da soli nel tempo.

### Cosa vogliamo ottenere

* scheduler persistente con trigger:

  * at time
  * interval
  * cron-like
* workflow schedulabili come background jobs
* stato persistito del job
* retry / backoff / timeout
* wake-up di processi o workflow senza intervento umano
* base per agenti demone locali

### Perché è prioritario

Perché è il passo che trasforma AgenticOS da runtime reattivo a sistema davvero autonomo.

### DoD

* [x] job scheduler persistito
* [x] workflow lanciabile da trigger temporale
* [x] stato job osservabile in GUI
* [x] retry/backoff/timeout per job

---

## M37) Control Center & Deep Observability

**Status:** `DONE`

### Obiettivo

Fare della GUI un vero centro di controllo del runtime.

### Cosa vogliamo ottenere

* vista chiara di:

  * stato processi
  * task workflow
  * runtime/backend/model
  * queue/in-flight
  * tool calls
  * request remote
  * timeout / retry
  * artifacts
* timeline chat pulita
* pannello tecnico separato per:

  * tool execution
  * runtime events
  * diagnostics
  * errors
* monitor dell’albero dei processi / workflow execution

### Perché è prioritario

Perché orchestrazione e proactive agents senza osservabilità trasformano il sistema in una black box ingestibile.

### DoD

* [x] chat separata da pannello tecnico
* [x] runtime state non ambiguo in GUI
* [x] workflow monitor leggibile
* [x] debugging remoto e tool plane osservabile

---

## M31) Protocol Contracts v2 & Workflow Control API

**Status:** `DONE`

### Obiettivo

Rendere i contratti control-plane più stabili e adatti a workflow/scheduler/artifacts.

### Cosa vogliamo ottenere

* envelope/versioning più espliciti
* schema JSON stabili per:

  * workflow definition
  * workflow status
  * artifacts
  * scheduler jobs
  * runtime diagnostics
* capability negotiation coerente
* base per futuri gateway esterni / API di integrazione

### Perché è importante

Perché quando workflow e scheduler diventano di primo livello, il protocollo deve rifletterli in modo pulito e machine-readable.

### DoD

* [x] schema stabili per orchestration/scheduler/artifacts
* [x] control-plane coerente con le nuove entità
* [x] additive-first evolution policy documentata

---

## M32) Episodic Memory & Semantic Retrieval

**Status:** `DONE`

### Obiettivo

Passare dal retrieval pragmatico attuale a una memoria episodica semanticamente utile.

### Cosa vogliamo ottenere

* retrieval semantico locale
* ranking migliore del contesto richiamato
* controllo costo/latenza
* memory relevance misurabile
* integrazione con workflow e agenti pro-attivi

### Perché viene dopo

Perché è importante, ma prima bisogna far emergere bene orchestrazione, artifacts e scheduler.
La memoria semantica diventa molto più utile quando il sistema esegue workload autonomi complessi e persistenti.

### DoD

* [x] retrieval semantico integrato
* [x] metriche utili su qualità retrieval
* [x] cost controls / latency controls
* [x] visibilità in STATUS / GUI

---

## M38) Human-in-the-Loop nativo

**Status:** `DONE`

### Obiettivo

Integrare punti di sospensione/approvazione umana nel runtime.

### Cosa vogliamo ottenere

* syscall/tool di tipo `ask_human`
* processo che si sospende senza consumare risorse inutilmente
* ripresa del workflow su risposta umana
* supporto nativo a approval steps nei workflow

### Perché è molto importante

Perché rende AgenticOS adatto a task reali con autonomia governata.

### DoD

* [x] wait state HITL supportato nel kernel
* [x] notifica/risposta dalla GUI
* [x] ripresa pulita del processo/workflow
* [x] approvazioni integrabili nei workflow

---

# Kill-features candidate (post-core)

## M39) Workflow Templates Library

**Status:** `TODO`

### Obiettivo

Rendere l’orchestrazione usabile subito senza richiedere all’utente di progettare DAG da zero.

### Esempi

* planner → worker → reviewer
* research pipeline
* multi-model compare
* codebase analyst
* summarize a directory/project

### Perché è una kill-feature

Perché rende immediatamente sfruttabile il motore di orchestrazione e abbassa enormemente la soglia d’ingresso.

---

## M40) IPC evoluto / Message Bus tipizzato

**Status:** `TODO`

### Obiettivo

Evolvere la semplice `ACTION:send` in un meccanismo di comunicazione più robusto e tracciabile.

### Cosa vogliamo ottenere

* mailbox o canali tipizzati
* passaggio strutturato di messaggi/eventi tra agenti
* tracciamento dei flussi di comunicazione
* integrazione con workflow e task outputs

### Perché viene dopo artifacts

Perché prima vogliamo una base artifact-first; l’IPC avanzato va aggiunto solo dopo.

---

## M41) Hybrid Local/Cloud Orchestration

**Status:** `TODO`

### Obiettivo

Sfruttare pienamente il routing multi-runtime già presente per orchestrare task su backend diversi.

### Cosa vogliamo ottenere

* planner locale + heavy reasoner cloud + finalizer locale
* policy runtime-aware per task graph
* ottimizzazione costi/latenza/qualità tra backend diversi

### Perché è una kill-feature

Perché trasforma AgenticOS in un vero sistema operativo per workload AI eterogenei, non in un semplice frontend multi-provider.

---

## M42) Browser / Computer Use sicuro

**Status:** `TODO`

### Obiettivo

Introdurre tool di navigazione/interazione web come capability di primo livello, in sandbox.

### Cosa vogliamo ottenere

* browser automation isolata
* interazione con pagine/form
* scraping visivo/strutturato
* governance forte di sicurezza

### Perché è una kill-feature

Perché apre il sistema al “mondo reale”, ma solo dopo che tool permissions, HITL e observability sono mature.

---

## M43) Visual Workflow Builder

**Status:** `TODO`

### Obiettivo

Costruire una UI visuale per comporre workflow e inspectare execution graph.

### Perché viene tardi

Perché prima bisogna stabilizzare:

* workflow model
* artifacts
* scheduler
* control-plane
* observability

Il visual builder ha senso solo sopra primitive già solide.

---

# Ordine di implementazione consigliato

## Priorità immediata

1. **M34 — Orchestrazione come feature di primo livello**
2. **M30 — Process-scoped permissions / supervisor boundaries**
3. **M35 — Artifacts & structured task I/O**
4. **M37 — Control Center & deep observability**

## Subito dopo

5. **M36 — Scheduler & proactive agents**
6. **M31 — Protocol Contracts v2**
7. **M32 — Semantic episodic memory**
8. **M38 — Human-in-the-loop**

## Kill-features successive

9. **M39 — Workflow templates**
10. **M40 — IPC evoluto**
11. **M41 — Hybrid local/cloud orchestration**
12. **M42 — Browser / computer use**
13. **M43 — Visual workflow builder**

---

# Perché questa è la sequenza giusta

Perché prima dobbiamo:

* esporre davvero l’orchestrazione,
* metterle i confini giusti,
* dare struttura agli output,
* osservare bene cosa succede,
* solo dopo schedulare agenti autonomi,
* e solo in un secondo tempo costruire le grandi killer features sopra una base già governabile.

Se vuoi, nel prossimo messaggio posso trasformare questa proposta in una **versione ancora più pronta da incollare nella ROADMAP**, con stile coerente al file attuale (`Status`, `Obiettivo`, `DoD`, `Validazione`).


> Tutte le milestone future dovranno essere implementate con il massimo rigore ingegneristico, rispettando sistematicamente le best practice di sviluppo software: pulizia architetturale, responsabilità ben separate, moduli piccoli e coesi, interfacce esplicite, error handling chiaro, niente duplicazioni inutili e niente complessità accidentale.
> La progettazione e l’implementazione dovranno mantenere il sistema il più semplice possibile rispetto agli obiettivi reali, introducendo nuovi file e moduli quando serve a separare correttamente la logica, evitando monoliti, scorciatoie fragili e accoppiamenti inutili.
