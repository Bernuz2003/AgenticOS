# AgenticOS GUI (MVP)

Interfaccia desktop PySide6 per controllare il kernel e osservare il runtime in tempo reale.

## Funzioni MVP

- Gestione kernel integrata: start/stop.
- Comandi rapidi: `PING`, `STATUS`, `LIST_MODELS`, `MODEL_INFO`, `GET_GEN`, `SHUTDOWN`.
- Pannello guidato Modelli: refresh `LIST_MODELS` JSON, selezione da menu a tendina (auto-discovery), `SELECT_MODEL`, `LOAD`, `MODEL_INFO` JSON, `BACKEND_DIAG` JSON.
- Pannello guidato Generation: `GET_GEN` + `SET_GEN` con campi `temperature/top_p/seed/max_tokens`.
- Comandi custom protocollo (`VERB payload`).
- `EXEC` in streaming con render live dei frame `DATA raw`.
- Hint workload Chat coerente col kernel: se l'utente seleziona `fast/code/reasoning/general`, la GUI inoltra `capability=<hint>;` al prompt; `auto` non aggiunge nulla.
- Azioni processo: `TERM <pid>`, `KILL <pid>`.
- Osservabilità:
  - polling `STATUS` ogni 2s,
  - eventi runtime da stdout/stderr kernel,
  - tail di `workspace/syscall_audit.log`.
- Hardening UX:
  - retry/reconnect automatico per richieste control/stream,
  - filtri live su eventi kernel e audit syscall,
  - export snapshot diagnostico in `reports/gui_snapshot_<timestamp>.txt`.

## Avvio

Dalla root del workspace:

```bash
python3 -m pip install -r gui/requirements.txt
python3 -m gui.app
```

## Note

- La GUI usa il protocollo TCP locale su `127.0.0.1:6380` (configurabile via `AGENTIC_PORT`).
- Ogni connessione viene autenticata automaticamente leggendo `workspace/.kernel_token` e inviando `AUTH` sul socket appena aperto.
- Se `target/release/agentic_os_kernel` esiste, viene usato per l’avvio kernel; altrimenti fallback a `cargo run --release`.
- `Stop Local Kernel` ferma il processo avviato dalla GUI (livello OS), mentre `Kernel SHUTDOWN` invia il comando protocollo di spegnimento graceful.
- `Stop PID (TERM)` richiede chiusura gentile del processo agentico; `Kill PID (KILL)` forza la chiusura immediata.
- `Refresh Runtime Status` aggiorna stato runtime (`active_pids`, errori, uptime, memoria, stato modello).
- `LIST_MODELS` e `MODEL_INFO` sono payload JSON machine-readable; la GUI non usa regex per ricostruire il catalogo.
- `BACKEND_DIAG` interroga `/health`, `/props` e `/slots` del backend esterno `llama.cpp` e mostra un riepilogo leggibile nella sezione Models.
- `MEMW` e' presentato come tool diagnostico low-level: payload canonico `<pid>\n<raw-bytes>`, con rifiuto esplicito dei body non allineati a 4 byte.
- `RESTORE` e' metadata-only: reapplica scheduler state, selected model hint e generation config, ma non ripristina processi live, pesi, tensori o output buffer.
- Le metriche Chat finali arrivano dal kernel (`tokens_generated`, `elapsed_secs`); durante lo streaming la GUI etichetta le stime come `approx`.
- `LOAD` usa un timeout esteso lato GUI (fino a 180s) per evitare falsi negativi durante il caricamento di modelli grandi.
- I codici errore control distinguono `TIMEOUT` da `MALFORMED_OR_PARTIAL` per semplificare il troubleshooting.
- Per mostrare i log `New connection` nel terminale kernel, abilita `AGENTIC_LOG_CONNECTIONS=1` (default: off).
- Lo scheduler di auto-switch modello su `EXEC` è ora **opt-in** (`AGENTIC_EXEC_AUTO_SWITCH=1`); default: disabilitato per evitare reload inattesi.
- `STATUS` espone sempre `selected_model_id` e `loaded_model_id`: la GUI mostra entrambi nel pannello `Control`.

## Smoke test manuale E2E (runbook)

1. Avvia GUI e premi `Start Kernel`.
2. Premi `PING` e verifica `+OK PING` nel log comandi.
3. Premi `Refresh LIST_MODELS`, seleziona un modello dalla lista, poi `SELECT_MODEL`.
4. Premi `LOAD selected` e verifica risposta `+OK`.
5. Premi `Backend Diag` nella sezione Models e verifica il report JSON del backend esterno quando `AGENTIC_LLAMACPP_ENDPOINT` e' configurato.
6. In Generation premi `GET_GEN`, modifica un valore e premi `SET_GEN`; verifica eco dei valori.
7. In `Exec` invia un prompt breve e controlla stream output + marker di fine processo.
8. Nella sezione Memory prova `MEMW` solo come strumento diagnostico low-level; verifica che payload disallineati restituiscano errore esplicito.
9. Premi `RESTORE` su snapshot valido e verifica nel pannello Memory il riepilogo metadata-only (clear/apply + limiti del restore).
10. Esegui `TERM`/`KILL` su PID valido e verifica risposta.
11. Verifica tab `Behind the scenes`:
  - eventi kernel popolati,
  - tail di `workspace/syscall_audit.log` popolato,
  - filtri testuali funzionanti.
12. Premi `Export snapshot` e verifica presenza file in `reports/`.
13. Premi `SHUTDOWN` oppure `Stop Kernel` e verifica chiusura pulita.
