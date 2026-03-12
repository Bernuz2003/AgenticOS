# Refactoring Plan — Local/Cloud Backend Separation

## Obiettivi

- separare nettamente le logiche `resident_local` e `remote_stateless`
- rendere i backend cloud model-aware e data-driven
- eliminare hardcode di provider/modelli dalla GUI
- preparare il kernel a nuovi provider senza moltiplicare rami speciali

## Fase 1 — Catalogo remoto data-driven

- [x] Introdurre un file dedicato `config/providers/remote_providers.toml` come sorgente di verita' per provider e modelli cloud supportati.
- [x] Aggiungere nel kernel il loader del catalogo remoto con fingerprint dedicato e refresh integrato nel `ModelCatalog`.
- [x] Estendere `LIST_MODELS` / `ModelCatalogSnapshot` con `remote_providers`.
- [x] Validare `LOAD cloud:<provider>[:<model>]` contro il catalogo remoto invece di usare provider/modelli hardcoded.
- [x] Aggiornare la Lobby Tauri/React con dropdown dinamici `Provider` -> `Modello`.
- [x] Rimuovere ogni opzione cloud hardcoded residua dal frontend.

## Fase 2 — Separazione moduli backend

- [x] Ridurre `crates/agentic-kernel/src/backend/mod.rs` a facade e trait surface.
- [x] Creare `crates/agentic-kernel/src/backend/local/` per `llama.cpp` residente.
- [x] Creare `crates/agentic-kernel/src/backend/remote/` per provider cloud/API.
- [x] Spostare telemetry, readiness e config lookup remoti fuori da `backend/mod.rs`.
- [x] Spostare diagnostics e loader locali fuori da `backend/mod.rs`.
- [x] Riallineare test e import sui nuovi moduli.

## Fase 3 — Target di load tipizzato

- [x] Introdurre `ResolvedLoadTarget::{Local, Remote}` al posto del path sintetico come source of truth.
- [x] Definire `RemoteLoadTarget` con `provider_id`, `backend_id`, `model_id`, `model_spec`, `runtime_config`.
- [x] Separare `display_path` da `runtime_reference`.
- [x] Eliminare il parsing secondario di stringhe `cloud/...` nei backend runtime.

## Fase 4 — Provider runtime model-aware

- [x] Unificare il runtime config cloud in un modello comune per provider remoti.
- [x] Consentire metadati modello-specifici: context window, max output, pricing, structured output.
- [x] Propagare questi metadati a telemetry, policy e GUI.
- [x] Associare ogni provider a un adapter esplicito (`openai_compatible`, futuri `anthropic`, ecc.).

## Fase 5 — GUI e contratti

- [x] Estendere i DTO con `loaded_target_kind`, `loaded_provider_id`, `loaded_remote_model_id`.
- [x] Mostrare in Lobby il modello remoto selezionato senza dipendere dal parsing del path.
- [x] Mostrare metadata modello cloud utili: contesto, max output, pricing.
- [x] Riutilizzare il catalogo remoto anche in eventuali pannelli futuri di orchestrazione.

## Fase 6 — Orchestrator backend-aware

- [x] Rendere l'orchestrator consapevole della classe backend richiesta dal task.
- [x] Introdurre policy di routing esplicite `resident_local` vs `remote_stateless`.
- [x] Evitare che task cloud ricevano semantiche di residency non supportate.
- [x] Preparare il secondo adapter cloud senza nuovi hardcode trasversali.

## Iterazione corrente

- [x] Formalizzare il piano di refactoring in un documento operativo.
- [x] Implementare la prima slice concreta: catalogo remoto data-driven + GUI guidata dal backend.
- [x] Avviare la seconda slice: estrazione moduli `backend/local` e `backend/remote`.
- [x] Completare la terza slice: target di load tipizzati `Local/Remote` e runtime reference disaccoppiata dal display path.
- [x] Completare la quarta slice: runtime cloud model-aware con adapter espliciti, limiti/prezzi per modello e surfacing in lobby.
- [x] Completare la quinta slice: contratti di stato canonici per target caricati e utility frontend riusabili per il catalogo remoto.
- [x] Completare la sesta slice: orchestrator backend-aware con gate espliciti di backend class e spawn cloud/local coerenti.
