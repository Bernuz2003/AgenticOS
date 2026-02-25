# AgenticOS GUI (MVP)

Interfaccia desktop PySide6 per controllare il kernel e osservare il runtime in tempo reale.

## Funzioni MVP

- Gestione kernel integrata: start/stop.
- Comandi rapidi: `PING`, `STATUS`, `LIST_MODELS`, `MODEL_INFO`, `GET_GEN`, `SHUTDOWN`.
- Pannello guidato Modelli: refresh `LIST_MODELS`, selezione da menu a tendina (auto-discovery), `SELECT_MODEL`, `LOAD`, `MODEL_INFO`.
- Pannello guidato Generation: `GET_GEN` + `SET_GEN` con campi `temperature/top_p/seed/max_tokens`.
- Comandi custom protocollo (`VERB payload`).
- `EXEC` in streaming con render live dei frame `DATA raw`.
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

- La GUI usa il protocollo TCP locale su `127.0.0.1:6379` (configurabile in alto).
- Se `target/release/agentic_os_kernel` esiste, viene usato per l’avvio kernel; altrimenti fallback a `cargo run --release`.

## Smoke test manuale E2E (runbook)

1. Avvia GUI e premi `Start Kernel`.
2. Premi `PING` e verifica `+OK PING` nel log comandi.
3. Premi `Refresh LIST_MODELS`, seleziona un modello dalla lista, poi `SELECT_MODEL`.
4. Premi `LOAD selected` e verifica risposta `+OK`.
5. In Generation premi `GET_GEN`, modifica un valore e premi `SET_GEN`; verifica eco dei valori.
6. In `Exec` invia un prompt breve e controlla stream output + marker di fine processo.
7. Esegui `TERM`/`KILL` su PID valido e verifica risposta.
8. Verifica tab `Behind the scenes`:
  - eventi kernel popolati,
  - tail di `workspace/syscall_audit.log` popolato,
  - filtri testuali funzionanti.
9. Premi `Export snapshot` e verifica presenza file in `reports/`.
10. Premi `SHUTDOWN` oppure `Stop Kernel` e verifica chiusura pulita.
