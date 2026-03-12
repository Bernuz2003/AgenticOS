# ACTION PLAN

## Obiettivo di questo step

Questo documento traduce i 6 problemi riportati in un piano operativo approvabile prima di qualunque modifica al codice. Per ogni punto descrivo:

- root cause tecnica;
- strategia di fix a livello di file/modulo;
- impatti architetturali;
- note di validazione.

## Guardrail architetturali

- Il kernel resta un processo TCP separato dalla GUI Tauri.
- Non introduco in questo step un transport embedded.
- Le fix devono preservare la separazione tra:
  - lifecycle della sessione/PID;
  - backend di inferenza locale o remoto;
  - bridge Tauri/UI.
- Per il punto 6 il design deve lasciare aperta la strada a:
  - sessioni residenti/background;
  - backend cloud/API;
  - orchestrazione autonoma con processi che si risvegliano senza input GUI.

## Ordine consigliato di esecuzione

1. Correggere il bootstrap dei path del workspace.
2. Correggere il timeout di `LOAD` e la UX di loading model.
3. Unificare il flusso UI `Select` / `Load`.
4. Rendere esplicita la diagnostica driver/backend per i modelli.
5. Correggere lo streaming a batch partendo dal backend esterno.
6. Rifattorizzare il lifecycle da job effimero a sessione residente.

---

## 1. Bootstrap Error: Swap Worker Path Mismatch

### Root cause

La validazione del path di swap usa la `current_dir()` del processo in `crates/agentic-kernel/src/memory/swap_io.rs`, costruendo implicitamente il workspace root come `cwd/workspace`.

Dopo il refactor a workspace multi-crate:

- `config.rs` risolve correttamente i path di default rispetto alla repository root;
- ma `swap_io::resolve_valid_swap_dir()` continua a usare la working directory runtime;
- se il kernel viene lanciato da `crates/agentic-kernel-bin`, il workspace atteso diventa `.../crates/agentic-kernel-bin/workspace`;
- il path richiesto da config resta invece `.../workspace/swap`;
- la validazione fallisce perché confronta due root diverse.

In altre parole: il bug non è nello `swap_dir` configurato, ma nel fatto che il validatore non usa la stessa source of truth dei path kernel.

### Strategia di fix

- Eliminare da `swap_io::resolve_valid_swap_dir()` ogni dipendenza da `std::env::current_dir()`.
- Usare come base `kernel_config().paths.workspace_dir` oppure passare esplicitamente il workspace root al validatore.
- Canonicalizzare la root reale del workspace configurato e validare che `swap_dir` stia sotto quella root.
- Allineare la logica con quella già usata da `tools/path_guard.rs`, che oggi usa correttamente `kernel_config().paths.workspace_dir`.

### File coinvolti

- `crates/agentic-kernel/src/memory/swap_io.rs`
- `crates/agentic-kernel/src/memory/swap.rs`
- `crates/agentic-kernel/src/config.rs`
- `crates/agentic-kernel/src/tools/path_guard.rs`
- test in `crates/agentic-kernel/src/memory/core.rs`

### Fix previsto nel codice

- Introdurre un helper condiviso per "workspace root canonico" invece di duplicare logica tra swap/tool path safety.
- Far sì che `resolve_valid_swap_dir()` validi contro `config.paths.workspace_dir`.
- Aggiornare i test che oggi assumono implicitamente `cwd == repo root`.

### Validazione

- `cargo run --release` da `crates/agentic-kernel-bin/`
- test memory/swap esistenti
- smoke test di bootstrap da repo root e da crate dir

---

## 2. Falso Positivo "Failed to load model" durante `LOAD`

### Root cause

Il problema è nel bridge Tauri, non nel comando `invoke` JavaScript.

Il path attuale è:

- UI -> Tauri command `load_model`
- Tauri Rust -> `KernelBridge::load_model()`
- `KernelBridge::send_control_command()` usa `read_single_frame(..., Duration::from_secs(5))`
- il `LOAD` del kernel è sincrono e blocca il loop finché `LLMEngine::load_target()` non termina
- se il modello impiega più di 5 secondi, il bridge va in timeout, chiude la connessione e propaga errore alla UI
- il kernel però continua a caricare il modello e termina con successo

La UI mostra quindi un errore falso perché il timeout del bridge è troppo corto per un'operazione intrinsecamente lunga.

### Strategia di fix

- Introdurre timeout per opcode, non un timeout unico di 5 secondi.
- Tenere timeout brevi per `AUTH`, `HELLO`, `PING`, `STATUS`.
- Dare a `LOAD` un timeout lungo e configurabile, o nessun timeout di lettura lato bridge per la sola fase di attesa del risultato.
- Mantenere la UI in stato `Loading...` finché la future Tauri non si risolve davvero, senza convertire il timeout in errore applicativo.

### File coinvolti

- `apps/agent-workspace/src-tauri/src/kernel/client.rs`
- `apps/agent-workspace/src-tauri/src/kernel/protocol.rs`
- `apps/agent-workspace/src-tauri/src/commands/kernel.rs`
- `apps/agent-workspace/src/pages/lobby-page.tsx`
- opzionalmente `agenticos.toml` / `crates/agentic-kernel/src/config.rs` se esponiamo timeout bridge configurabili

### Fix previsto nel codice

- Estrarre una policy timeout nel bridge Tauri:
  - short timeout per control-plane leggero;
  - long timeout per `LOAD`.
- Lasciare `actionLoading === "load"` attivo in UI fino alla risposta reale.
- Rendere il messaggio UI più esplicito:
  - "Loading model..."
  - "Il kernel sta caricando il modello, attendi"
- Non mostrare `Failed to load model` su timeout fittizi del bridge.

### Nota importante

Il bottone `Load` oggi ha già uno stato locale `Loading...`; il problema è che il bridge lo tronca dopo 5 secondi. Quindi la fix primaria è sul bridge, non sul frontend React.

### Validazione

- test manuale con modello pesante > 5s
- la UI deve restare in loading continuo fino a `+OK LOAD`
- nessun alert rosso se il kernel completa correttamente il load

---

## 3. UX/Architecture: `Select Model` vs `Load Model`

### Root cause

Nel kernel i due comandi hanno semantica diversa, ma in UI oggi sembrano due step obbligatori di uno stesso workflow:

- `SELECT_MODEL`
  - aggiorna solo `model_catalog.selected_id`
  - emette eventi `ModelChanged` / `LobbyChanged`
  - non tocca `engine_state`
  - non carica pesi né tokenizer

- `LOAD`
  - risolve il target effettivo
  - carica `LLMEngine`
  - aggiorna il modello realmente disponibile per `EXEC`
  - se il target ha un `model_id`, sincronizza anche `selected_id`

Quindi oggi `Select` è un hint/catalog state; `Load` è l'operazione reale di attivazione runtime.

### Conclusione architetturale

Per l'utente finale della GUI Tauri, `Select` è ridondante.

Ha ancora un valore interno/protocollo:

- fallback per `LOAD` senza selector;
- default per `MODEL_INFO`;
- persistenza del catalog selection nei checkpoint metadata-only;
- compatibilità con client futuri/CLI.

Ma nella UI primaria genera solo confusione.

### Strategia di fix

- Unificare il flusso della Lobby in un solo bottone `Load`.
- Il `select` dal dropdown resta uno stato locale del form, non un'operazione remota separata.
- Il kernel può continuare a supportare `SELECT_MODEL` per backward compatibility.
- Dopo `LOAD(selectedDraft)`, il kernel aggiorna già `selected_model_id`, quindi il modello selezionato e quello caricato restano coerenti.

### File coinvolti

- `apps/agent-workspace/src/pages/lobby-page.tsx`
- `apps/agent-workspace/src/lib/api.ts`
- `apps/agent-workspace/src-tauri/src/lib.rs`
- `apps/agent-workspace/src-tauri/src/commands/kernel.rs`
- `crates/agentic-kernel/src/commands/model.rs`

### Fix previsto nel codice

- Rimuovere dalla UI primaria il bottone `Select`.
- Tenere il dropdown.
- Far puntare `Load` sempre al valore selezionato nel dropdown.
- Lasciare il syscall/opcode `SELECT_MODEL` disponibile ma de-enfatizzato o deprecato lato UI.

### Validazione

- caricamento da Lobby con un solo click operativo
- `selected_model_id` e `loaded_model_id` coerenti dopo `LOAD`
- nessuna regressione su `MODEL_INFO` e checkpoint

---

## 4. Diagnostica Performance Load: Qwen3.5 vs Llama3.1 / Qwen2.5

### Root cause

Dal codice attuale la risposta più probabile è: non è un effetto di `mmap`.

Motivi:

- i loader Candle usati da AgenticOS (`quantized_llama` e `quantized_qwen2`) non usano `mmap` per il GGUF;
- il vecchio loader nativo rimosso leggeva i tensori con `read_exact()` in memoria;
- quindi i path locali fanno materializzazione esplicita dei tensori dal file, non memory mapping lazy.

La spiegazione più plausibile, in base al repo attuale, è un'altra:

- `Qwen3.5` è documentato nel repo come `family=Qwen`, `architecture=qwen35`;
- il driver locale Candle supporta `qwen2`, non `qwen35`;
- se `AGENTIC_LLAMACPP_ENDPOINT` è configurato, il resolver può instradare `qwen35` verso `external-llamacpp`;
- il backend `external-llamacpp` non apre il file GGUF nel kernel: istanzia solo un adapter HTTP;
- di conseguenza il `LOAD` percepito è quasi immediato, perché il kernel non sta facendo il lavoro di ingest dei pesi che invece fa per `llama` o `qwen2`.

### Nota di inferenza

Questa diagnosi è una inferenza forte basata sul codice e sulla config attuale del repo:

- `agenticos.toml` contiene `external_llamacpp.endpoint = "http://127.0.0.1:8080"`;
- l'architettura `qwen35` è esplicitamente trattata come non compatibile con backend locali incompatibili;
- il repo contiene test che confermano il fallback a `external-llamacpp` quando l'endpoint RPC è disponibile.

Se il tuo file reale `Qwen3.5` dichiarasse invece `general.architecture=qwen2`, allora la diagnosi cambierebbe e andrebbe confrontata con un benchmark locale puro. Ma il codice del progetto oggi punta chiaramente alla prima interpretazione.

### Strategia di fix / adeguamento

Questo punto è più diagnostico che correttivo, ma propongo due adeguamenti:

- rendere sempre visibili in UI:
  - `architecture`
  - `resolved_backend`
  - `driver_resolution_source`
- arricchire il risultato di `LOAD` e/o `MODEL_INFO` con un indicatore operativo chiaro:
  - `in_process`
  - `remote_adapter`

Così l'utente capisce subito se sta confrontando:

- un load locale resident backend;
- oppure un attach a backend esterno.

### File coinvolti

- `crates/agentic-kernel/src/backend/mod.rs`
- `crates/agentic-kernel/src/model_catalog/formatting.rs`
- `crates/agentic-kernel/src/services/model_runtime.rs`
- `apps/agent-workspace/src/pages/lobby-page.tsx`
- `apps/agent-workspace/src/lib/api.ts`

### Validazione

- `MODEL_INFO` / Lobby devono mostrare chiaramente backend e architettura
- confronto prestazionale solo tra modelli caricati dallo stesso tipo di backend

---

## 5. Streaming a scatti: batch di ~16 token

### Root cause

Per il caso Qwen3.5 il collo di bottiglia più probabile è il backend esterno, non il bridge Tauri.

Il path attuale del driver RPC fa questo:

- `ExternalLlamaCppBackend::generate_step()`
- calcola `chunk_tokens = min(remaining, config.external_llamacpp.chunk_tokens)`
- invia una richiesta `/completion` con:
  - `n_predict = chunk_tokens`
  - `stream = false`
- riceve un blocco di testo/tokens
- restituisce quel blocco come un singolo `InferenceStepResult`
- il runtime emette un singolo `KernelEvent::TimelineChunk`
- la GUI visualizza quel blocco tutto insieme

Quindi il batching nasce già nel driver backend.

Il bridge Tauri oggi non sta raggruppando i chunk della timeline:

- `KernelEvent::TimelineChunk` viene gestito subito in `apps/agent-workspace/src-tauri/src/kernel/events.rs`;
- `emit_timeline_snapshot()` parte immediatamente;
- non c'è debounce sul timeline stream.

### Conclusione

Per il sintomo descritto, il problema non è:

- né l'inference worker locale;
- né il socket event bridge Tauri;

ma il fatto che il backend esterno chiede e restituisce più token per step con `stream=false`.

### Strategia di fix

#### Fix corretto

- Estendere il backend `external-llamacpp` a uno streaming vero:
  - usare `stream=true` verso il backend remoto;
  - parsare gli eventi/chunk incrementali;
  - trasformarli in delta di testo/token nel runtime.

#### Fallback minimo

- Ridurre `external_llamacpp.chunk_tokens` a `1`.

Questo però è solo un workaround:

- aumenta drasticamente il numero di round-trip HTTP;
- non scala bene;
- non è il design giusto per futuri backend cloud.

### Adeguamento architetturale consigliato

Per non spostare il collo di bottiglia dal backend al bridge/UI:

- mantenere nel kernel un evento delta-oriented;
- evitare di serializzare e riemettere l'intero `TimelineSnapshot` ad ogni token;
- far append lato store/frontend, non ricostruire sempre tutta la timeline.

Questo è importante soprattutto per:

- future API cloud;
- sessioni lunghe;
- agenti proactive in background.

### File coinvolti

- `crates/agentic-kernel/src/backend/external_llamacpp.rs`
- `crates/agentic-kernel/src/backend/http.rs`
- `crates/agentic-kernel/src/backend/remote_adapter.rs`
- `crates/agentic-kernel/src/backend/mod.rs`
- `crates/agentic-kernel/src/inference_worker.rs`
- `crates/agentic-kernel/src/runtime/inference_results.rs`
- `apps/agent-workspace/src-tauri/src/kernel/events.rs`
- `apps/agent-workspace/src-tauri/src/kernel/stream.rs`
- `apps/agent-workspace/src/app/layout.tsx`
- `apps/agent-workspace/src/store/workspace-store.ts`

### Validazione

- output visualizzato token-by-token o chunk molto piccoli e regolari
- assenza di burst da 16/32 token per Qwen3.5
- nessuna regressione per il backend locale residente `llama.cpp`

---

## 6. Lifecycle: da `fire-and-forget` a `continuous chat`

### Root cause

L'architettura attuale modella ogni `EXEC` come un job effimero:

- il processo genera fino a stop condition;
- `inference_worker` imposta `ProcessState::Finished`;
- `runtime/orchestration::handle_finished_processes()`:
  - emette `SessionFinished`;
  - rilascia memoria e scheduler metadata;
  - chiama `kill_managed_process()`;
- il PID sparisce da `engine.processes`;
- `STATUS` non lo riporta più nella Lobby;
- la Timeline Tauri lo marca come chiuso definitivo;
- non esiste nessun opcode per inviare nuovo input allo stesso PID.

Quindi oggi manca il concetto di sessione residente distinta dal singolo turno di inferenza.

### Root cause architetturale profonda

Il runtime confonde tre concetti diversi:

- lifecycle del PID/sessione;
- lifecycle del singolo turno di generazione;
- lifecycle del task orchestrato.

Finché questi tre concetti coincidono, il modello è inevitabilmente "spawn -> run -> kill".

### Strategia di fix

#### 1. Separare session lifecycle da turn lifecycle

Introdurre uno stato residente esplicito, ad esempio:

- `WaitingForInput` o `Idle`

distinto da:

- `Ready` = runnable / schedulabile subito
- `Running`
- `Parked`
- `WaitingForSyscall`
- `Finished`

#### 2. Introdurre una policy di lifecycle per processo

Serve una policy, non una regola globale unica:

- `Ephemeral`
  - comportamento attuale
  - adatto a task orchestration/DAG
- `Interactive`
  - dopo la risposta passa a `WaitingForInput`
  - mantiene contesto, slot memoria e PID

Questo evita di rompere l'orchestrator, che oggi dipende dal fatto che un task "finito" venga davvero completato e rimosso.

#### 3. Nuovo comando di follow-up su PID esistente

Serve un nuovo opcode/control path per "manda un nuovo prompt a questo PID":

- append della nuova user turn nel contesto del processo esistente
- reset dello stato a runnable
- nessun respawn
- stesso PID

Questa è la vera capability mancante per la continuous chat.

#### 4. Non killare le sessioni interactive al termine del turno

`handle_finished_processes()` dovrà:

- per sessioni `Ephemeral`: comportamento attuale
- per sessioni `Interactive`:
  - non chiamare `kill_managed_process()`
  - non rilasciare memoria/context slot
  - passare a `WaitingForInput`
  - emettere un evento di fine turno / cambio stato, non un "process morto"

#### 5. Rifattorizzare Timeline/UI

La timeline Tauri oggi è modellata come:

- un solo prompt iniziale;
- uno stream assistant;
- una chiusura finale.

Per supportare multi-turn serve un log append-only di turni:

- user turn 1
- assistant turn 1
- user turn 2
- assistant turn 2
- ecc.

#### 6. Preservare la porta aperta per proactive e cloud

La nuova entità da preservare non è "un processo che sta sempre generando", ma "una sessione residente con PID stabile".

Questo si adatta bene a:

- agente che dorme e si risveglia;
- swap su disco;
- backend locale o remoto;
- routing futuro verso API cloud.

### File coinvolti

- `crates/agentic-kernel/src/process.rs`
- `crates/agentic-kernel/src/engine/lifecycle.rs`
- `crates/agentic-kernel/src/services/process_runtime.rs`
- `crates/agentic-kernel/src/commands/exec.rs`
- nuovo command/opcode nel layer `crates/agentic-kernel/src/commands/`
- `crates/agentic-kernel/src/runtime/inference_results.rs`
- `crates/agentic-kernel/src/runtime/orchestration.rs`
- `crates/agentic-kernel/src/services/status_snapshot.rs`
- `crates/agentic-kernel/src/checkpoint.rs`
- `crates/agentic-kernel/src/scheduler.rs`
- `crates/agentic-protocol/src/lib.rs`
- `protocol/schemas/v1/*.json`
- `crates/agentic-control-models/src/lib.rs`
- `apps/agent-workspace/src-tauri/src/commands/kernel.rs`
- `apps/agent-workspace/src-tauri/src/kernel/client.rs`
- `apps/agent-workspace/src-tauri/src/kernel/events.rs`
- `apps/agent-workspace/src-tauri/src/kernel/stream.rs`
- `apps/agent-workspace/src/lib/api.ts`
- `apps/agent-workspace/src/pages/workspace-page.tsx`
- componenti workspace/lobby per composer e card sessione

### Fix previsto nel codice

- aggiungere `ProcessLifecyclePolicy` o equivalente al processo/spawn request
- aggiungere `WaitingForInput`/`Idle`
- introdurre un nuovo control command per follow-up su PID
- emettere evento typed di `session_state_changed` o `turn_completed`
- rendere la Timeline store multi-turn
- lasciare residenti in RAM/Swap le sessioni interactive
- mantenere `Ephemeral` per orchestrazione, così la DAG resta semanticamente corretta

### Rischi da gestire esplicitamente

- con il modello single-engine attuale, sessioni residenti bloccheranno il `LOAD` di un altro modello finché non vengono terminate
- la UI dovrà esporre chiaramente il concetto di sessione attiva ma idle
- checkpoint/restore resta metadata-only: possiamo persistere il nuovo stato, ma non promettere live restore completo in questo step

### Validazione

- una sessione diretta da Lobby risponde e poi resta visibile in stato `Idle` / `WaitingForInput`
- lo stesso PID riceve un secondo prompt e continua con contesto già presente
- orchestrazioni continuano a completarsi e rilasciare risorse come oggi
- `STATUS`, Lobby e Workspace riflettono correttamente i nuovi stati

---

## Sintesi finale

Il piano proposto corregge:

- un bug reale di risoluzione path introdotto dal refactor multi-crate;
- un timeout falso-positivo nel bridge Tauri;
- una UX confusa su `Select` vs `Load`;
- una lettura fuorviante delle performance di `Qwen3.5`, che con alta probabilità dipendono dal routing verso backend esterno e non da `mmap`;
- il batching dello streaming, che nasce nel driver RPC con `stream=false` e `chunk_tokens > 1`;
- il modello di sessione effimera, separando finalmente turno, sessione e task orchestrato.

Se approvi questo piano, il passo successivo è implementarlo nell'ordine sopra, partendo dai punti 1-3 perché sono i più localizzati e sbloccano rapidamente i test manuali sul kernel e sulla GUI.
