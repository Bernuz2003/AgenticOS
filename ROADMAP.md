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
- GUI: PySide6 con control center per modelli/processi/memoria/orchestrazione/tooling

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
- Invocazione canonica `TOOL:<name> <json>` + alias legacy instradati via registry.
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
- GUI Python: `python3 -m compileall gui` verde nell'ultimo passaggio M29

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
