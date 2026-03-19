# AgenticOS — Roadmap Operativa (Compattata)

Questa roadmap e' la fonte unica di verita' per il piano corrente.
Il dettaglio storico fine-grained vive nella git history e nei documenti di critica (`CRITICITY_TO_FIX.md`).

## Regole di mantenimento

- Tenere qui solo: stato corrente, decisioni architetturali attive, milestone in corso/prossime, archivio sintetico.
- Evitare log narrativi lunghi per milestone concluse.
- Aggiornare ogni milestone con stati `TODO` -> `IN_PROGRESS` -> `DONE`.
- Ogni milestone deve avere DoD verificabile e validazione esplicita.

---

## Snapshot corrente

- Data snapshot: 2026-03-10
- Versione progetto: `v0.5.0`
- Runtime: TCP event-driven (`mio`) + inference worker + kernel process-centric
- Focus prodotto: AI workstation OS local-first, single-node
- Test Rust: `cargo test` verde (`166 passed, 0 failed, 1 ignored`)
- Qualita': clippy verde con `-D warnings` nell'ultima validazione M28/M29
- GUI: `Agent Workspace` (Tauri) come workspace primario; PySide6 in fallback diagnostico `deprecated`
- Cloud onboarding Fase 6 estesa: oltre a `openai-responses`, il kernel supporta ora `groq-responses` e `openrouter` tramite runtime remoto OpenAI-compatible condiviso; resta aperto solo il routing orchestrator backend-aware.
- Config hardening in corso: bootstrap layered su `config/kernel` + `config/env` per separare file pubblici versionabili e segreti locali push-safe.

---

## Decisioni architetturali attive

1. Il framing TCP locale resta invariato; evoluzione su payload/contratti machine-readable.
2. `RESTORE` resta metadata-only: non promette live process restore/KV cache restore.
3. Tooling: registry dinamico + policy esplicite; no hardcode crescente nel syscall plane.
4. Context management: policy per processo, osservabile via `STATUS`, integrata con orchestration.
5. Scope breve termine: robustezza kernel locale e control plane; no salto immediato a distributed/Tokio migration totale.

---

## Roadmap attiva

### M28) Tool registry dinamico
**Status:** `DONE` (con hardening incrementale)

**Obiettivo**
- Registry tool dinamico e tipizzato, con discovery/control-plane JSON e dispatch syscall canonico.

**Completato**
- `ToolDescriptor` + `ToolBackendConfig` + `ToolRegistry` in-memory con bootstrap built-in.
- OpCodes: `REGISTER_TOOL`, `UNREGISTER_TOOL`, `LIST_TOOLS`, `TOOL_INFO`.
- Invocazione canonica separata per piani distinti: `TOOL:<name> <json-object>` per il tool plane e `ACTION:<name> <json-object>` per il runtime/process-control plane.
- Manifest dinamico tool/action derivato dal registry reale e usato nel bootstrap dei nuovi agenti.
- Path structured interno parallelo al parser testuale, convergente sulla stessa `ToolInvocation` e sullo stesso dispatcher.
- Backend `remote_http` con policy di sicurezza (allowlist, timeout, payload/header checks).
- Integrazione GUI completa (list/info/register/unregister) + test parser/client.
- Validazione: `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, test Python GUI.

**Hardening residuo (non bloccante)**
- Formalizzare ulteriormente policy per permessi/scope per processo (integrazione con scheduler/orchestration).
- Rafforzare governance compatibilita' semantica per endpoint legacy dove serve.

---

### M29) Context window management
**Status:** `DONE`

**Obiettivo**
- Rendere la gestione contesto per PID una capability strutturale del process model (non un accessorio di `EXEC`).

**Completato**
- Process model esteso con `ContextStrategy`, `ContextPolicy`, `ContextState`, `ContextSegment`.
- Hint additive in `EXEC` (`context_strategy`, `context_window`, `context_trigger`, `context_target`).
- `sliding_window` con enforcement pre-step e reset coerente del context slot backend.
- `summarize` come compaction event non bloccante.
- `retrieve` pragmatico con store episodico serializzabile e hit accounting.
- Override espliciti della context policy nei task `ORCHESTRATE` JSON, propagati fino agli spawn request runtime.
- Ranking retrieval evoluto a ibrido lessicale+recency; non e' ancora semantic retrieval pieno, ma non e' piu' FIFO puro.
- Campi context additive in `STATUS` globale/per-PID/per-orchestration.
- Schema control-plane e GUI Orchestration aggiornati per mostrare policy context e snapshot dei task running.
- Checkpoint/restore metadata-only estesi per policy e stato context.

**Chiusura milestone**
- Test long-running multi-turn aggiunti sul process model per `sliding_window`, `summarize` e `retrieve`, con verifica di bound, compaction e osservabilita'.
- `ARCHITECTURE.md` e `gui/README.md` riallineati ai nuovi contratti di context policy per `EXEC`/`ORCHESTRATE` e allo snapshot context in `STATUS orch:`.

**Validazione di chiusura**
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `python3 -m compileall gui`

---

## Prossimi task candidati (post M29)

### M33) Agent Workspace (Tauri GUI Rewrite)
**Status:** `DONE`

**Perche'**
- La GUI PySide6 attuale resta utile come fallback diagnostico, ma non e' piu' adeguata come workspace primario per osservabilita' agentica, sessioni e workflow inspection.

**Target**
- Nuova app desktop Tauri con frontend web moderno (`React + TypeScript + Tailwind`) e backend Rust come bridge TCP autenticato verso il kernel.
- UX a due livelli: `Lobby` sessioni/agenti a card + `Workspace` con timeline interattiva e `Mind Panel` telemetrico.
- Event model tipizzato nel bridge desktop, senza dipendere da parsing fragile del testo raw del kernel.
- Deprecazione progressiva della GUI PySide6 solo dopo parita' minima verificata.

**Slice completata**
- Estratta la crate condivisa `crates/agentic-protocol` e primo riuso kernel-side di header/opcode/envelope/schema.
- Scaffold iniziale creato in `apps/agent-workspace/` con shell Tauri, frontend `React + TypeScript + Tailwind`, routing `Lobby -> Workspace` e component skeleton non-demo.
- Bridge Tauri evoluto da placeholder a client TCP persistente per `AUTH`/`HELLO`/`STATUS`, con primo snapshot Lobby agganciato ai dati reali del kernel (`active_processes`).
- Workspace agganciata a `STATUS <pid>` per il `Mind Panel`, con polling live di budget contesto, strategy, compressions e retrieval hits.
- CTA `Nuova sessione` collegata a `EXEC` reale via bridge Tauri, con apertura immediata della Workspace sul PID appena creato.
- Timeline Workspace alimentata da stream `EXEC` reale per le sessioni avviate dalla nuova GUI, con buffer eventi per PID (`user_message`, chunk assistant, `PROCESS_FINISHED`).
- Strategia di fallback introdotta per PID nati fuori dal bridge Tauri: la Workspace sintetizza una timeline degradata da `STATUS <pid>` e audit events, marcata esplicitamente come fallback.
- Timeline assistant rifinita con renderer Markdown e autoscroll coerente; `Mind Panel` aggiornato con audit events strutturati/human-readable e feedback visuale per compaction.
- Timeline live ora normalizza anche i marker runtime gia' esistenti per `thinking` (`<think>...</think>`) e `tool_call` (`[[...]]`), senza estendere ancora il protocollo del kernel.
- Timeline arricchita con eventi `tool_result` di prima classe derivati dall'audit log per PID.
- `STATUS` globale esteso con indice delle orchestration attive e payload per-PID estesi con metadata orchestration/task.
- Lobby e Workspace rese orchestration-aware: aggregati live in Lobby e dettaglio task/policy in Workspace per PID orchestrati.
- Validazione desktop Tauri completata con `npm exec tauri build -- --debug` dopo installazione librerie Linux richieste.

**Chiusura milestone**
- `Agent Workspace` e' ora la GUI desktop primaria del progetto.
- PySide6 resta disponibile ma viene mantenuta come fallback diagnostico `deprecated` fino a un ulteriore ciclo operativo sul campo.
- Resta fuori scope M33 una capability protocollo/kernel di attach o replay su stream `EXEC` gia' esistenti; il fallback `STATUS <pid>` resta il comportamento ufficiale per PID esterni al bridge.

### M30) Process-scoped Tool Permissions
**Status:** `TODO`

**Perche'**
- Il registry da solo offre estensibilita'; serve governabilita' per PID/orchestrazione.

**Target**
- Policy per processo/task su: tool allowlist, path scope, timeout, budget syscall/costo, livello trust.
- Enforcement runtime uniforme + audit coerente.
- Inheritance policy: kernel default -> orchestration default -> task override.

---

### M31) Protocol Contracts v2 (negoziabili)
**Status:** `TODO`

**Target**
- Envelope versionato e capability negotiation esplicita.
- Schema JSON stabili per tutti gli endpoint control-plane (status/diag/tools/orchestration/backend).
- Strategia additive-first + compat policy documentata.

---

### M32) Episodic Memory & Semantic Retrieval
**Status:** `TODO`

**Target**
- Evolvere il retrieval M29 da recency-based a semantico (ranking migliore, quality guardrail, cost controls).
- Misure osservabili in `STATUS` per precisione/recall operativa e impatto su latenza.

---

## Archivio milestone completate (sintesi)

| Fase | Milestone | Stato | Output principale |
|---|---|---|---|
| Foundation | M1-M12 | DONE | Stabilita' runtime/protocollo, scheduler, memory, GUI base, hardening architetturale |
| Consolidamento | M13-M17 | DONE | Swap extraction, checkpoint/restore metadata-only, orchestrator DAG, benchmark+OOM fix |
| GUI Redesign | M10.1-M10.2 | DONE | Sidebar+sezioni dedicate, copertura completa opcode control-plane |
| Criticita' | C1-C21 | DONE | Chiusura criticita' alte/medie: concorrenza, protocol/GUI drift, bound orchestrazione, trust operatore |
| Workstation hardening | M18-M20 | DONE | Contratti coerenti, GUI fidelity, context discipline su orchestration |
| Future-model flexibility | M21-M25 | DONE | Backend abstraction, metadata runtime-first, capability routing v2, pilot Qwen3.5 |
| Microkernel boundary | M26-M27 | DONE | Context slots astratti + driver RPC esterno `llama.cpp` + backend diagnostics |

Note: il dettaglio completo delle sub-task concluse e' mantenuto nella cronologia git e nei documenti di review/criticita'.

---

## Stato validazione (ultimo giro)

- Rust tests: verdi (`166 passed, 0 failed, 1 ignored`)
- Clippy: verde (`-D warnings` su all-targets/all-features)
- Frontend Tauri: `cd apps/agent-workspace && npm run build` verde
- Desktop Tauri: `cd apps/agent-workspace && npm exec tauri build -- --debug` verde
- GUI Python: `python3 -m compileall gui` verde nell'ultimo passaggio M29

---

## Aggiornamento operativo 2026-03-17

- Refactor affidabilita'/osservabilita' runtime+GUI completato sul workspace Tauri e kernel control-plane:
	- tracciamento tool reso deterministico con `tool_call_id` end-to-end (dispatch/completion/failure) e timeline Tauri agganciata ad audit persistito di sessione;
	- stati runtime resi piu' espliciti in UI (`runtime_state` esposto in Lobby/Workspace, incluso `AwaitingRemoteResponse`);
	- osservabilita' remote path ampliata con audit dedicato `remote.request_started|completed|failed` e `duration_ms` su accounting event;
	- semantica `delete session` riallineata a `terminate + remove` (tentativo TERM/KILL live PID, purge live timeline, delete persistito);
	- system prompt globale rafforzato contro tool-use superfluo (nessuna policy differenziata per backend/modello).

- Validazione eseguita:
	- `cargo test` verde.
	- `cd apps/agent-workspace && npm run build` verde.
	- `cd apps/agent-workspace && npm exec tauri build -- --debug` verde.
	- `cargo clippy --all-targets --all-features -- -D warnings` non verde per issue preesistenti fuori scope slice + warning dead-code nel modulo legacy `apps/agent-workspace/src-tauri/src/kernel/audit.rs`.

- Hotfix regressione `delete_session` (single-source-of-truth alignment):
	- root cause: il path Tauri `delete_session` deduceva il PID da `fetch_lobby_snapshot()` (view ibrida live+persisted) usando anche `last_pid` storico;
	- effetto: tentativi `TERM/KILL` su PID non piu live -> errore `NO_MODEL`/`TERM_KILL_FAILED` in UI;
	- fix: `delete_session` ora consulta solo il live state kernel (`STATUS.active_processes`) per decidere terminate;
	- guard race-safe: se tra check e terminate il processo e gia uscito, `NO_MODEL`/`PID_NOT_FOUND` viene trattato come terminal state gia raggiunto;
	- SQLite resta la verita persistita per la cancellazione storica (`history_db::delete_session` invariato).

- Validazione hotfix regressione:
	- `cargo test -p agentic-kernel` verde (`264 passed, 0 failed, 1 ignored`).
	- `cd apps/agent-workspace/src-tauri && cargo test` verde (`22 passed, 0 failed`).

## Aggiornamento operativo 2026-03-18

- Avviato refactor strutturale runtime/remote/sync con completamento della **Fase A (Discovery + mappatura)**.
- Deliverable prodotto: `docs/runtime_refactor_phaseA_discovery_2026-03-18.md` con:
	- evidenza del loop attuale basato su polling fisso (`poll_timeout_ms`, default 5ms) e assenza di wakeup esplicito worker->event-loop;
	- evidenza del path remoto attuale con lettura stream aggregata prima della propagazione runtime chunk;
	- classificazione formale source-of-truth: kernel in-memory (live) + SQLite (persisted), con cache/view/debug derivati;
	- mappa duplicazioni/merge fragili nel bridge Tauri (timeline/live/persisted/audit) e lista file target per Fasi B/C/D/E.
- Stato piano operativo: Fase A marcata completata in `docs/runtime_refactor_coerente_plan_2026-03-18.md`.

- **Fase B (event loop deadline-driven + wakeup esplicito)** completata su kernel runtime:
	- introdotto planner esplicito delle deadline (`remote_timeout`, `syscall_timeout`, `retry_backoff`, `maintenance_cleanup`, `scheduled_work`, `checkpoint`) con modulo dedicato `crates/agentic-kernel/src/runtime/deadlines.rs`;
	- loop kernel refactorato da timeout fisso a timeout calcolato per `poll` in funzione della prossima deadline reale + heartbeat fallback lento;
	- introdotto wakeup esplicito worker->event-loop via `mio::Waker` (inference worker + syscall worker), eliminando la dipendenza dal tick fisso per il processamento dei risultati pronti;
	- aggiunta osservabilita minima del motivo wakeup (`network`, `worker`, `deadline`, `heartbeat_fallback`) con contatori per eventi tick processati;
	- introdotto timeout handling esplicito per syscall wait e reporting controlled per remote timeout in-flight (senza spin loop).

- Validazione Fase B:
	- `cargo test -p agentic-kernel` verde (`264 passed, 0 failed, 1 ignored`).

- **Fase C (streaming remoto progressivo reale)** completata su backend/runtime path:
	- introdotto `stream_observer` nel contratto di inferenza (`InferenceStepRequest`) per propagare chunk incrementali durante lo step;
	- backend remoto OpenAI-compatible rifattorizzato da decode buffer-first a decode incrementale per read-chunk, con emissione live dei delta mentre lo stream e ancora aperto;
	- inference worker aggiornato con evento intermedio `InferenceResult::StreamChunk` (con flag first-chunk) e deduplicazione del testo finale per separare nettamente partial stream vs completion finale;
	- runtime aggiornato per inoltro immediato chunk a client owner + `KernelEvent::TimelineChunk`, mantenendo separato l evento finale di completamento (`InferenceResult::Token` -> lifecycle esistente);
	- telemetria remota minima completata: `request_started`, `first_chunk_received`, `request_completed`/`request_failed`, `duration_ms`.
	- cleanup tecnico effettuato: rimosso path legacy `run_step` nel worker in favore di pipeline unica streaming-aware.

- Validazione Fase C:
	- `cargo test -p agentic-kernel` verde (`264 passed, 0 failed, 1 ignored`).
	- Verifica UX live workspace confermata (streaming progressivo reale end-to-end).

- **Fase D (sync rigoroso live vs persisted)** avviata con primo slice strutturale su bridge Tauri:
	- `fetch_workspace_snapshot(session_id, pid=None)` portata in modalita live-first: risoluzione preventiva PID live da `STATUS.active_processes` e fetch diretto snapshot live prima di qualsiasi fallback SQLite;
	- rimosso merge live+persisted nel path `KernelBridge::fetch_workspace_snapshot(pid)`: lo snapshot live non viene piu arricchito con campi storici potenzialmente stantii;
	- introdotta regola anti-overwrite per audit: eventi audit persisted usati solo come fallback se il live snapshot non ha audit, evitando sostituzioni non deterministiche del tracciato live;
	- timeline live (`source=live_exec`) resa autoritativa sia nel command `fetch_timeline_snapshot` sia nel bridge eventi: niente augment ex-post da audit SQLite durante stream attivo.

- Validazione slice Fase D:
	- `cargo test -p agent-workspace` verde (`22 passed, 0 failed`).

- **Fase D (slice 2: D1 + D3 + D5)** consolidata su bridge/workspace sync:
	- naming/ruoli esplicitati nel modulo kernel Tauri con alias semantici: `persisted_truth` (SQLite storico), `live_cache` (timeline live in-memory), `debug_audit` (debug-only);
	- guard anti-contaminazione `session_id<->pid` introdotte nei fetch snapshot/timeline: i path live/fallback ora verificano coerenza di sessione prima di accettare snapshot runtime;
	- recovery storico mantenuta deterministicamente su `persisted_truth` (SQLite) quando il live non e disponibile;
	- `delete_session` resa definitiva per sessione logica: terminate di tutti i PID live associati alla sessione, delete persistito e purge cache timeline sia per PID sia per `session_id`.

- Validazione slice 2 Fase D:
	- `cargo test -p agent-workspace` verde (`22 passed, 0 failed`).

- **Fase D chiusa** con completamento D6 e validazione consolidata:
	- GUI resa trasparente sul runtime state: separazione esplicita `session status` vs `runtime state` nelle view Session/Workspace, senza mascherare stati runtime specifici;
	- completata dedup/guard dei fetch live nel bridge con check sistematico `session_id<->pid` e fallback deterministici su SQLite storico;
	- semantica delete session confermata atomica su sessione logica (terminate multi-PID live + purge cache + delete persistito).
	- validazione finale: `cargo test -p agent-workspace` verde (`22 passed, 0 failed`), `cargo test -p agentic-kernel` verde (`264 passed, 0 failed, 1 ignored`), `cd apps/agent-workspace && npm run build` verde.

- **Fase E avviata** (slice iniziale E1/E3/E5):
	- deduplicata la logica di derivazione stato sessione nel frontend (`deriveSessionStatus`) e introdotti helper centralizzati per label/tone runtime;
	- deduplicata nel bridge la logica di fetch live snapshot con helper riusabili, riducendo branching duplicato e mantenendo le invarianti della Fase D;
	- migliorata leggibilita dei path principali con separazione semantica e minore complessita accidentale.

- **Fase E completata** (slice finale E1/E2/E4/E6):
	- rimosse euristiche fragili nel path persisted: niente inferenza PID da `session_id` (`pid-*`) in `history_db`; la risoluzione PID usa solo campi persistiti/hint espliciti;
	- aggiunto test di regressione su store storico per impedire reintroduzione della derivazione PID da naming (`lobby_sessions_do_not_infer_pid_from_session_id_text`);
	- ridotta duplicazione residua nel fallback session UI (`WorkspacePage`) con builder sintetico condiviso;
	- ridotto stato temporaneo ridondante nello `workspace-store` (rimozione `activePid` interno non necessario e mantenimento di stato derivato invalidabile).

- Validazione Fase E:
	- `cargo test -p agent-workspace` verde (`23 passed, 0 failed`).
	- `cargo test -p agentic-kernel` verde (`264 passed, 0 failed, 1 ignored`).
	- `npm run build` (`apps/agent-workspace`) verde.
	- `cargo clippy --all-targets --all-features -- -D warnings` eseguito: non verde per issue preesistenti e trasversali fuori scope dello slice (es. `needless_borrow` in `commands/model.rs`, `too_many_arguments` in moduli storici, doc-style lint, alcuni `collapsible_if`).

- **Fase F completata** (hardening finale + quality gate):
	- risolti warning/lint clippy nel kernel su path core e test (`collapsible_if`, `needless_borrow`, `derivable_impls`, `ptr_arg`, `bool_assert_comparison`, `let_and_return`, `type_complexity`) e applicate annotazioni mirate `#[allow(clippy::too_many_arguments)]` sulle API intenzionalmente aggregate;
	- aggiornato il piano operativo `docs/runtime_refactor_coerente_plan_2026-03-18.md` marcando F1/F2/F3/F4, DoD e criteri globali come completati;
	- mantenuta la nota governance: `CRITICITY_TO_FIX.md` attualmente assente nel repository, quindi nessun aggiornamento file-specifico possibile.

- Validazione Fase F:
	- `cargo clippy -p agentic-kernel --all-targets -- -D warnings` verde.
	- `cargo test -p agentic-kernel` verde (`264 passed, 0 failed, 1 ignored`).
	- `cargo test -p agent-workspace` verde (`23 passed, 0 failed`).
	- `cd apps/agent-workspace && npm run build` verde.

---

## Template aggiornamento milestone

```md
### MX) Titolo
**Status:** `TODO|IN_PROGRESS|DONE`

**Obiettivo**
- ...

**DoD**
- [ ] ...
- [ ] ...

**Validazione**
- test/command: ...
- evidenze: ...
```
