# Agent Workspace - Piano Operativo di Migrazione GUI

## Milestone proposta

### M33) Agent Workspace (Tauri GUI Rewrite)
**Status:** `DONE`

**Obiettivo**
- Deprecare progressivamente la GUI PySide6 e introdurre una nuova GUI desktop basata su Tauri, orientata a sessioni, osservabilita' cognitiva e workflow inspection.
- Mantenere invariato il kernel AgenticOS come server TCP locale; la nuova app desktop agisce da bridge Rust tipizzato verso il frontend web.

**Vincoli non negoziabili**
- Nessuna modifica architetturale che spinga il kernel verso Tokio o HTTP-first: il control plane resta TCP locale.
- `RESTORE` continua a essere metadata-only anche nella nuova GUI.
- La GUI PySide6 viene deprecata, non rimossa subito: resta fallback operativo finche' la nuova GUI non raggiunge una parita' minima verificata.
- Il frontend non deve dipendere da parsing regex o da testo raw del kernel per le primitive principali della UI.

**Outcome atteso**
- Un'app desktop denominata `Agent Workspace` con due superfici principali:
  - `Lobby`: hub sessioni/agenti a card.
  - `Workspace`: timeline interattiva + mind panel telemetrico per PID/sessione.

---

## Valutazione della proposta UX

La direzione e' corretta e coerente con lo stato del progetto:

- L'attuale GUI PySide6 e' utile per controllo e debug, ma non esprime il valore di M28/M29.
- La metafora `Workflow Inspector` e' piu' adatta del vecchio modello `task manager + pannelli tecnici`.
- La separazione `Timeline` / `Mind Panel` e' forte perche' riflette due livelli reali del kernel: output dell'agente e stato cognitivo osservabile.

Punti da preservare nel design esecutivo:

- Le card della Lobby devono rappresentare una `sessione di lavoro` con PID runtime associato, non solo un PID nudo.
- Il blocco `Thinking` va mostrato solo quando il backend espone reasoning distinguibile.
- I blocchi `Tool/Syscall` devono nascere da eventi normalizzati dal bridge Tauri, non da parsing fragile di testo libero.
- Il pannello audit deve offrire eventi umani ad alto livello, filtrati lato bridge/backend desktop.

---

## Stack scelto

### Desktop shell
- `Tauri v2`

### Frontend
- `React`
- `TypeScript`
- `Vite`
- `Tailwind CSS`

### Librerie frontend consigliate
- `React Router` per navigazione Lobby -> Workspace.
- `Zustand` per stato applicativo leggero e store reattivi per sessioni, timeline e telemetria.
- `TanStack Query` solo per query/polling control-plane se utile; non come sostituto del domain store.
- `react-markdown` per render Markdown finale.
- `Radix UI` come base accessibile per accordion, dialog, toast e tooltip.
- `Lucide` per iconografia.

### Motivazione della scelta
- React + TypeScript e' la scelta piu' pragmatica per una UI ricca di stato, timeline dinamica, pannelli telemetrici e componenti altamente composti.
- Tauri consente backend desktop Rust vicino al kernel senza introdurre un server HTTP intermedio non necessario.
- Tailwind accelera il design system iniziale, ma la UI dovra' avere una direzione visiva custom e non sembrare un admin dashboard generico.

---

## Strategia workspace Tauri

### Posizionamento nel repo

La nuova app non deve vivere nella root del crate kernel. Il piano e' creare una sottodirectory dedicata:

```text
apps/
  agent-workspace/
    package.json
    src/
    src-tauri/
    tauri.conf.json
```

Motivazione:

- Evita collisioni con il crate Rust del kernel gia' presente in root.
- Mantiene separati runtime kernel e runtime desktop.
- Consente build e packaging indipendenti.

### Bootstrap previsto

Comando previsto al momento dello scaffolding:

```bash
mkdir -p apps
cd apps
npm create tauri-app@latest agent-workspace
```

Template previsto:

- Frontend: `React + TypeScript`
- Package manager: `npm`
- Styling: integrazione `Tailwind CSS` subito dopo il bootstrap

Nota operativa:

- Non eseguiamo ancora lo scaffolding in questa fase.
- Al momento dell'implementazione verificheremo la sintassi esatta della CLI Tauri disponibile localmente, per evitare drift tra versioni del generatore.

---

## Bridge Rust Tauri <-> Kernel AgenticOS

## Problema attuale

Oggi il protocollo del kernel e' un modulo interno al binario principale, non una crate condivisa. Quindi il bridge Tauri non puo' semplicemente `riusare la crate protocol` perche' quella crate ancora non esiste come unita' separata.

## Decisione proposta

Prima dello sviluppo UI sostanziale, estrarre una libreria condivisa minimale, ad esempio:

```text
crates/
  agentic-protocol/
```

Contenuto minimo della crate condivisa:

- framing/envelope TCP
- parsing opcode canonico
- helper auth handshake
- DTO serde per payload JSON stabili principali
- error model comune lato client/server dove sensato

### Ruolo del backend Tauri

Il backend Tauri sara' l'unico client TCP del frontend. Responsabilita':

- apertura e riuso connessione TCP locale verso il kernel
- lettura token da `workspace/.kernel_token`
- invio `AUTH` su nuova connessione
- comandi control-plane tipizzati (`PING`, `STATUS`, `STATUS <pid>`, `LIST_MODELS`, `MODEL_INFO`, `GET_GEN`, `SET_GEN`, `ORCHESTRATE`, `TERM`, `KILL`, `LOAD`, `SELECT_MODEL`)
- stream `EXEC` e trasformazione in eventi frontend consumabili
- polling periodico dei dati runtime e telemetrici
- lettura e filtraggio alto livello di `workspace/syscall_audit.log`

### Struttura consigliata del backend Tauri

```text
apps/agent-workspace/src-tauri/src/
  main.rs
  app_state.rs
  commands/
    mod.rs
    kernel.rs
    sessions.rs
    telemetry.rs
  kernel/
    client.rs
    protocol.rs
    auth.rs
    stream.rs
    audit.rs
    mapping.rs
  models/
    session.rs
    timeline.rs
    telemetry.rs
    orchestration.rs
```

### Contratti da esporre al frontend

Il backend Tauri non deve inoltrare semplicemente testo raw. Deve esporre eventi normalizzati, per esempio:

- `session_list_updated`
- `session_status_updated`
- `exec_chunk_received`
- `thinking_chunk_received`
- `tool_call_started`
- `tool_call_finished`
- `context_compaction_event`
- `audit_events_updated`
- `kernel_connection_state_changed`

Questo punto e' cruciale: la nuova UX richiede un event model, non una console mascherata.

---

## Modello dati UI

### Entita' principali

- `AgentSessionSummary`
  - `session_id`
  - `pid`
  - `title`
  - `prompt_preview`
  - `status`
  - `uptime_secs`
  - `tokens_generated`
  - `context_strategy`

- `AgentWorkspaceState`
  - `session`
  - `timeline_items[]`
  - `mind_panel`
  - `audit_events[]`

- `TimelineItem`
  - `UserMessage`
  - `ThinkingBlock`
  - `ToolCallBlock`
  - `ToolResultBlock`
  - `AssistantMessage`
  - `SystemEvent`

- `MindPanelTelemetry`
  - `context_tokens_used`
  - `context_window_size`
  - `context_strategy`
  - `context_compressions`
  - `context_retrieval_hits`
  - `last_compaction_reason`
  - `last_summary_ts`
  - `model_id`
  - `status`

### Nota importante sulla semantica

`session_id` e `pid` non devono essere trattati come sinonimi permanenti. Il PID e' un handle runtime; la sessione e' una primitiva UX piu' stabile.

---

## Struttura frontend

### Albero pagine

```text
src/
  main.tsx
  app/
    router.tsx
    providers.tsx
    layout.tsx
  pages/
    lobby-page.tsx
    workspace-page.tsx
    settings-page.tsx
  components/
    lobby/
      session-card.tsx
      new-agent-card.tsx
      session-grid.tsx
      lobby-header.tsx
    workspace/
      workspace-shell.tsx
      timeline-pane.tsx
      mind-panel.tsx
      audit-stream.tsx
    timeline/
      user-message.tsx
      assistant-message.tsx
      thinking-accordion.tsx
      tool-call-card.tsx
      tool-result-card.tsx
      system-event-row.tsx
    mind/
      context-gauge.tsx
      strategy-badge.tsx
      counters-strip.tsx
      compaction-toast.tsx
      telemetry-card.tsx
    shared/
      app-button.tsx
      status-badge.tsx
      empty-state.tsx
      loading-state.tsx
  store/
    sessions-store.ts
    workspace-store.ts
    connection-store.ts
  lib/
    api.ts
    events.ts
    format.ts
    markdown.ts
    theme.ts
```

---

## UX operativa per schermata

## Fase UX 1 - Lobby

### Requisiti
- Layout a griglia di card ampie e cliccabili.
- CTA primaria `Nuovo Agente / Nuova Chat` sempre visibile.
- Ogni card mostra:
  - titolo/session label
  - preview del prompt iniziale
  - PID corrente
  - stato con badge colorato
  - uptime
  - token consumati/generati
  - context strategy attiva se disponibile

### Fonte dati
- `STATUS` globale e dati per-PID gia' esposti dal kernel.

### Criticita' da evitare
- Non mostrare solo una tabella re-skinnata.
- Non legare la navigazione a refresh manuali continui.

## Fase UX 2 - Workspace

### Timeline (70%)
- Chat fluida con messaggi user a destra e agent a sinistra.
- `Thinking` renderizzato come accordion solo quando i chunk reasoning sono disponibili.
- `Tool/Syscall` renderizzati come blocchi tecnici distinti, con stato `running/success/error`.
- Risposta finale renderizzata in Markdown pulito.
- Eventi di sistema minori compattati in righe leggere e non invasive.

### Mind Panel (30%)
- Gauge visuale `context_tokens_used / context_window_size`.
- Strategy badge (`SlidingWindow`, `Summarize`, `Retrieve`).
- Counter badge per compressions e retrieval hits.
- Notifica visuale quando arriva un nuovo compaction event.
- Audit stream filtrato, leggibile e a scorrimento.

### Polling/aggiornamento
- Polling `STATUS <pid>` regolare per la telemetria.
- Event stream `EXEC` separato dalla telemetria periodica.
- Audit tail aggiornato su timer dedicato o file watch dove ragionevole.

---

## Piano esecutivo a fasi

## Fase 0 - Design contract e pre-migrazione
- [ ] Congelare la GUI PySide6 come fallback diagnostico ufficiale.
- [ ] Definire i DTO minimi che la nuova GUI deve consumare.
- [ ] Identificare gap protocol-level tra output raw attuale e bisogni della nuova timeline.
- [x] Decidere se introdurre una milestone roadmap separata M33 in `ROADMAP.md` dopo approvazione del piano.

## Fase 1 - Shared protocol extraction
- [x] Estrarre `agentic-protocol` dal modulo interno corrente o creare una crate condivisa minima.
- [x] Centralizzare framing, auth handshake e parsing opcode lato client per la prima verticale `AUTH`/`HELLO`/`STATUS`.
- [x] Aggiungere test Rust per roundtrip client/kernel sui comandi principali.

## Fase 2 - Bootstrap app Tauri
- [x] Creare `apps/agent-workspace/` con Tauri + React + TypeScript.
- [x] Integrare Tailwind CSS.
- [x] Configurare comandi `dev`, `build`, `tauri dev`, `tauri build`.
- [x] Definire naming, icona, packaging e config base.

## Fase 3 - Kernel bridge Tauri
- [x] Implementare client TCP persistente lato Tauri.
- [x] Leggere `workspace/.kernel_token` e autenticare ogni nuova connessione.
- [x] Mappare i primi command handlers tipizzati per control plane (`bootstrap_state`, `protocol_preview`, `fetch_lobby_snapshot`).
- [x] Introdurre canale eventi/frontend commands per stream `EXEC`, telemetria e audit.
- [x] Introdurre adapter placeholder che trasformano output raw in `TimelineItem` e `MindPanelTelemetry`.

## Fase 4 - Lobby UI
- [x] Implementare grid delle sessioni.
- [x] Implementare card stato con badge, preview, uptime e token.
- [x] Implementare CTA `Nuovo Agente / Nuova Chat`.
- [x] Collegare click card a routing Workspace.

## Fase 5 - Workspace UI
- [x] Implementare shell split-screen.
- [x] Implementare timeline message blocks.
- [x] Implementare blocchi `Thinking`, `ToolCall`, `ToolResult`, `AssistantMessage`.
- [x] Implementare Markdown renderer e gestione scroll coerente.
- [x] Implementare composer/input per invio prompt.

## Fase 6 - Mind Panel e audit
- [x] Implementare gauge context budget.
- [x] Implementare badge strategy, compression count, retrieval hits.
- [x] Implementare feedback visuale per compaction event.
- [x] Implementare audit stream human-readable con filtro lato bridge.

## Fase 7 - Orchestration-aware UX
- [x] La Lobby mostra anche orchestration attive come aggregati live.
- [x] La Workspace espone lo stato dei task orchestrati quando il PID nasce da orchestration.
- [x] La UX resta coerente con i campi M29 di `STATUS orch:`.

## Fase 8 - Parita' minima e deprecazione reale PySide6
- [x] Verificare parita' minima su start/stop kernel, `EXEC`, `STATUS`, modelli, orchestration, tooling, restore visibility.
- [x] Aggiornare documentazione root e roadmap.
- [x] Spostare PySide6 in stato `deprecated` ufficiale nel README.
- [x] Pianificare rimozione solo dopo almeno un ciclo completo di validazione operativa.

---

## Rischi principali

1. Costruire la timeline su testo raw invece che su eventi normalizzati.
2. Accoppiare troppo UI e PID senza introdurre il concetto di sessione.
3. Rimuovere troppo presto la GUI PySide6 e perdere superfici operative.
4. Fare Tauri app monolitica senza separare bene bridge Rust e frontend UI.
5. Sottostimare il lavoro necessario per distinguere `thinking`, `tool calls` e `final answer` in modo affidabile.

---

## Definition of Done della milestone M33

- [ ] Esiste una nuova app desktop `Agent Workspace` sotto `apps/agent-workspace/`.
- [ ] Il backend Tauri comunica col kernel via TCP autenticato senza introdurre un server intermedio ad hoc.
- [ ] Lobby e Workspace sono funzionanti e usabili.
- [ ] La Workspace mostra timeline agentica e mind panel con telemetria M29.
- [ ] Il bridge espone eventi tipizzati al frontend, senza regex parsing per le primitive core.
- [ ] La GUI PySide6 e' marcata `deprecated` ma resta disponibile come fallback fino a parita' minima verificata.
- [x] Esiste una nuova app desktop `Agent Workspace` sotto `apps/agent-workspace/`.
- [x] Il backend Tauri comunica col kernel via TCP autenticato senza introdurre un server intermedio ad hoc.
- [x] Lobby e Workspace sono funzionanti e usabili.
- [x] La Workspace mostra timeline agentica e mind panel con telemetria M29.
- [x] Il bridge espone eventi tipizzati al frontend, senza regex parsing per le primitive core.
- [x] La GUI PySide6 e' marcata `deprecated` ma resta disponibile come fallback fino a parita' minima verificata.

---

## Validazione prevista

### Rust
- `cargo test`
- test del crate condiviso protocol
- test backend Tauri per auth, framing e mapping eventi

### Frontend
- lint TypeScript
- test componenti principali
- smoke test navigation Lobby -> Workspace

### E2E desktop
- avvio app
- connessione kernel + auth riuscita
- `EXEC` con streaming funzionante
- aggiornamento telemetria `STATUS <pid>`
- rendering compaction/retrieval events
- lettura audit stream filtrato

## Stato della slice corrente

- Estratta la crate `crates/agentic-protocol` e riagganciato il kernel alle definizioni condivise di opcode/header/envelope/schema.
- Scaffold creata in `apps/agent-workspace/` con shell Tauri v2 e frontend `React + TypeScript + Tailwind` compatibile con Node 18.
- Sostituito il template demo con skeleton reale `Lobby` -> `Workspace`, routing e componenti non-demo.
- Implementato un bridge Tauri persistente con `AUTH`, `HELLO` e `STATUS`, piu' comando `fetch_lobby_snapshot` per la prima verticale live del control plane.
- La Lobby non usa piu' seed statici: le card sono alimentate dallo snapshot reale del kernel (`active_processes`) e mostrano stato, token, uptime e context strategy.
- Il `Mind Panel` della Workspace usa ora `STATUS <pid>` reale con polling periodico: budget contesto, strategy, compressions, retrieval hits e audit events ad alto livello derivati dal bridge.
- La CTA `Nuova sessione` avvia ora un vero `EXEC` dal bridge Tauri, crea un PID runtime reale e apre la Workspace corrispondente.
- La Timeline non e' piu' seedata per le sessioni avviate dalla nuova GUI: il backend Tauri bufferizza i frame `DATA raw`, propaga i chunk assistant e chiude la sessione sul marker `PROCESS_FINISHED` senza cambiare il protocollo kernel.
- La Timeline assistant renderizza ora Markdown reale con autoscroll coerente durante lo streaming, invece di plain text statico.
- Il `Mind Panel` espone ora audit events strutturati dal bridge (`runtime`, `status`, `compaction`, `summary`) con filtro UI e feedback visuale sui nuovi compaction events.
- La Timeline live normalizza ora anche blocchi `thinking` e `tool_call` dai marker reali del runtime (`<think>...</think>` e `[[...]]`), senza introdurre ancora nuove capability protocollo/kernel.
- La Timeline integra ora anche `tool_result` di prima classe, derivati da `workspace/syscall_audit.log` e correlati per PID al flusso della sessione.
- Il control plane espone ora un indice globale delle orchestration attive in `STATUS` e metadata `orchestration_id`/`orchestration_task_id` nei payload per-PID.
- La Lobby mostra aggregate card live delle orchestration attive; la Workspace mostra task corrente e snapshot coerente con `STATUS orch:<id>` per i PID orchestrati.

## Chiusura milestone

- Il frontend builda correttamente (`npm run build`).
- Il kernel resta verde su `cargo test` e `cargo clippy --all-targets --all-features -- -D warnings`.
- Il backend desktop Tauri compila correttamente sulla macchina Linux dopo l'installazione di `webkit2gtk-4.1` e `javascriptcoregtk-4.1` (`npm exec tauri build -- --debug`).
- La Timeline live copre le sessioni avviate dal bridge Tauri, ma non puo' ricostruire retroattivamente output di PID nati fuori dalla nuova GUI perche' il kernel non persiste lo stream `EXEC`.
- Per i PID nati fuori dal bridge Tauri e' ora previsto un fallback esplicito basato su `STATUS <pid>`: la Workspace mostra una timeline degradata con stato runtime e audit events, marcata come `status_fallback`.
- Questo fallback e' intenzionalmente limitato: oggi il protocollo/kernel non espone un opcode o una capability per attach a uno stream `EXEC` gia' esistente; in futuro si potra' introdurre una capability dedicata di stream attach/replay.
- La GUI PySide6 resta disponibile come fallback diagnostico, ma la GUI primaria del progetto e' ora `Agent Workspace`.

---

## Decisioni che richiedono la tua approvazione prima dello scaffolding

1. Confermare `React + TypeScript + Tailwind` come stack frontend.
2. Confermare creazione dell'app in `apps/agent-workspace/` e non nella root del repo.
3. Confermare l'estrazione preliminare di una crate/shared module `agentic-protocol` come prerequisito del bridge Tauri.
4. Confermare che PySide6 resti fallback diagnostico fino a raggiungimento della parita' minima.

Se questi quattro punti restano approvati, il passo successivo sara' iniziare lo scaffolding della Fase 1/Fase 2, non prima.