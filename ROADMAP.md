# AgenticOS — Roadmap Operativa

Questo file è la fonte unica di verità per il piano immediato del progetto.

## Come usarla

- Aggiornare lo stato di ogni punto a fine attività (`TODO` → `IN_PROGRESS` → `DONE`).
- Registrare data, note sintetiche e commit/riferimenti utili.
- Non aprire un nuovo punto senza una **Definition of Done (DoD)** verificabile.

---

## Stato attuale (snapshot)

- Data snapshot: **2026-02-22**
- Modello target: `models/Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf`
- Runtime: server TCP event-driven (`mio`) + engine LLM process-centric
- Verificato: `PING`, `LOAD`, `EXEC` funzionanti end-to-end

---

## Roadmap immediata

### 1) Stabilità core runtime
**Status:** `DONE` ✅

**Obiettivi**
- Flush affidabile di risposte e stream.
- Gestione robusta errori I/O lato socket.
- Cleanup processi terminati (no zombie PID).
- Segnale esplicito di fine processo verso client.

**DoD**
- [x] Output pending ⇒ socket registrato `WRITABLE`.
- [x] `ConnectionReset` / `BrokenPipe` gestiti con chiusura pulita.
- [x] Processi `Finished` ripuliti dal runtime loop.
- [x] Marker di completamento processo inviato al client.
- [x] Build `release` senza errori.

**Note implementative**
- `main.rs`: flush/reregister + cleanup finished PID + marker `[PROCESS_FINISHED ...]`.
- `engine.rs`: helper per owner lookup + lista processi finiti.

---

### 2) Hardening protocollo TCP
**Status:** `DONE` ✅

**Obiettivi**
- Parser opcode coerente e case-insensitive.
- Framing risposta più formale (header + payload length).
- Errori standardizzati (`-ERR` con codici).

**DoD (target)**
- [x] Parser accetta varianti previste e rifiuta input ambiguo.
- [x] Risposte di controllo con formato deterministico e codici (`+OK CODE ...`, `-ERR CODE ...`).
- [x] Suite test parser/protocollo verde.
- [x] Framing completo header+payload length per risposte dati/stream (`DATA raw <len>\r\n<payload>`).

---

### 3) Qualità inferenza Llama 3
**Status:** `DONE` ✅

**Obiettivi**
- Ridurre variabilità first-token/total-time.
- Migliorare completamento risposte.
- Parametri generation configurabili per request.

**DoD (target)**
- [x] Parametri sampling esposti runtime in modo model-agnostic (`GET_GEN`/`SET_GEN`).
- [x] Benchmark baseline replicabile con soglie realistiche.
- [x] Riduzione troncamenti nei prompt lunghi.

---

### 4) Hardening subsystem SysCall
**Status:** `TODO`

**Obiettivi**
- Esecuzione Python con timeout/isolamento minimo.
- Path safety robusta nel workspace.
- Logging strutturato su invocazioni tool.

**DoD (target)**
- [ ] Timeout processo esterno + gestione error path.
- [ ] Verifica anti path traversal robusta.
- [ ] Audit log syscall con outcome e durata.

---

### 5) Test professionali & regressione
**Status:** `DONE` ✅

**Obiettivi**
- Unit/integration test su protocollo, I/O, flow EXEC.
- Test multi-client e reconnect.
- Report benchmark versione/commit-aware.

**DoD (target)**
- [x] Test critici automatizzati base (protocol + transport framing + loopback TCP).
- [x] Regressioni principali coperte (multi-client, reconnect, disconnect, partial I/O, header error recovery).
- [x] Report benchmark ripetibile e commit-aware (`reports/benchmark_commit_aware.json`).

---

### 6) Osservabilità operativa
**Status:** `TODO`

**Obiettivi**
- Metriche minime runtime (latency/error/throughput).
- Logging strutturato con PID/client-id.
- Graceful shutdown.

**DoD (target)**
- [ ] Metriche base esportabili/stampabili.
- [ ] Log con campi consistenti per debugging.
- [ ] Shutdown senza perdita stato critica.

---

### 7) Refactoring modulare multi-LLM
**Status:** `IN_PROGRESS` 🚧

**Obiettivi**
- Separare responsabilità per evitare crescita incontrollata dei file core.
- Astrarre dipendenze model-family-specific (Llama/Qwen/Mistral).
- Abilitare selezione modello intuitiva via catalogo + autodiscovery.
- Preparare base per suite test incrementale.

**DoD (target)**
- [x] Catalogo modelli runtime con autodiscovery `.gguf` in `models/`.
- [x] Nuovi comandi protocollo: `LIST_MODELS`, `SELECT_MODEL`, `MODEL_INFO`.
- [x] `LOAD` retrocompatibile (path) + supporto modello selezionato (`LOAD` payload vuoto).
- [x] Astrazione prompt-family in modulo dedicato (`Llama/Qwen/Mistral/Unknown`).
- [x] Decoupling backend engine (rimozione coupling diretto a `quantized_llama::ModelWeights`).
- [x] Primo livello test incrementali (unit parser protocollo + catalog family inference).
- [x] Split task-specific dei moduli core (`main`, `commands`, `transport`, `runtime`, `tools`).
- [x] Moduli Rust sotto soglia `< 300` righe (snapshot corrente).
- [x] Test incrementali transport/framing (partial header/body, concatenated commands, error recovery).
- [ ] Supporto backend Qwen runtime.
- [ ] Suite test incrementale multi-livello completa (unit/integration/e2e).

**Note implementative**
- `src/model_catalog.rs`: discovery, selezione, info e risoluzione target di load.
- `src/prompting.rs`: formattazione system/user per famiglia modello.
- `src/protocol.rs`: opcodes estesi e fix parsing `MEMW` case-insensitive.
- `src/main.rs`: bootstrap e wiring del loop eventi (ridotto a ~120 righe).
- `src/commands.rs`: dispatch ed esecuzione opcodes protocollo.
- `src/transport.rs`: client state machine + read/write non bloccante.
- `src/runtime.rs`: scheduler processi + routing syscall/output streaming.
- `src/tools.rs`: tool syscall filesystem/python/calc.
- `src/backend.rs`: runtime backend astratto (`RuntimeModel`) con dispatch per famiglia.
- `src/engine.rs` + `src/process.rs`: refactor su backend astratto (no type coupling diretto sparso a Llama).

---

## Registro avanzamento

| Data       | Punto | Stato        | Note |
|------------|-------|--------------|------|
| 2026-02-22 | 1     | DONE         | Stabilità runtime implementata e verificata con smoke test. |
| 2026-02-22 | 7     | IN_PROGRESS  | Avviato refactoring: catalogo modelli + prompt abstraction + comandi protocollo modello. |
| 2026-02-22 | 7     | IN_PROGRESS  | Decoupling backend completato + test unitari iniziali + smoke test end-to-end su flusso catalog/load/exec. |
| 2026-02-22 | 7     | IN_PROGRESS  | Modularizzazione completata: split `main/commands/transport/runtime/tools`, build+test verdi, target `<300` righe raggiunto. |
| 2026-02-22 | 7     | IN_PROGRESS  | Aggiunti test incrementali `transport` su framing chunked/recovery; suite test totale verde (9 test). |
| 2026-02-22 | 2/5   | IN_PROGRESS  | Hardening protocollo (header strict + error code) + test loopback TCP su transport; suite test verde (15 test). |
| 2026-02-22 | 2     | DONE         | Protocollo hardening concluso: header strict, reply codificate con lunghezza, stream data framed, test verdi. |
| 2026-02-22 | 5     | IN_PROGRESS  | Estesa regressione TCP: aggiunti test multi-client + reconnect; suite verde (17 test). |
| 2026-02-22 | 5     | DONE         | Chiuso punto test/regressione: benchmark commit-aware integrato e report JSON ripetibile generato. |
| 2026-02-22 | 3     | IN_PROGRESS  | Avviata qualità inferenza model-agnostic: policy stop per-family + configurazione sampling runtime (`GET_GEN`/`SET_GEN`). |
| 2026-02-22 | 3     | DONE         | Baseline Meta-Llama-3.1 calibrata (timeout/soglie realistiche), parser stream framed robusto e stop su `PROCESS_FINISHED`; report aggiornato in `reports/llama3_eval_report.json`. |

---

## Template aggiornamento (copia/incolla)

```md
### X) Titolo punto
**Status:** `IN_PROGRESS`

**Obiettivi**
- ...

**DoD**
- [ ] ...
- [ ] ...

**Esito**
- ...

**Evidenze**
- file: ...
- test/command: ...
```
