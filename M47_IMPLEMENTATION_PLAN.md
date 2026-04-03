# M47 Implementation Plan

## Root Cause

### Problema 1
- I builder della timeline bridge-side filtravano i segmenti assistant vuoti, ma non i segmenti `Thinking`.
- Il seed storico poteva quindi reidratare thinking semanticamente vuoti e riproporli nel live timeline store.

### Problema 2
- Il system prompt era troppo minimale e non distingueva con sufficiente forza tra intenzione, evento reale kernel-side e testo assistant.
- Il projection layer era gia' corretto nel trattare gli eventi strutturati come source of truth, ma mancavano test espliciti sui casi di pseudo-output.

### Problema 3
- `STOP_OUTPUT` era modellato solo come conferma finale su `AwaitingTurnDecision`.
- Durante l'inferenza reale il processo viene checkoutato al worker, quindi non basta allargare una guardia di stato: serve una richiesta di stop soft persistita fino al check-in successivo.

### Problema 4
- La GUI non trasportava alcuna configurazione di quota alla creazione sessione.
- Il bridge Tauri usava ancora un payload ambiguo per `EXEC`.
- Le quote scheduler e il budget tecnico del turno erano accoppiati nello spawn path.

### Problema 5
- Dopo `SEND_INPUT`, il bridge Tauri poteva seedare la live timeline da SQLite con il nuovo turno gia' persistito e poi appenderlo di nuovo nel live store.
- La duplicazione non era un problema di rendering GUI ma di composizione dati bridge-side tra persisted truth e stato live.

## File Toccati

- `NEXT_FIX.md`
- `M47_IMPLEMENTATION_PLAN.md`
- `apps/agent-workspace/src/pages/chats/page.tsx`
- `apps/agent-workspace/src/pages/chats/detail.tsx`
- `apps/agent-workspace/src/components/workspace/composer/controls.tsx`
- `apps/agent-workspace/src/components/workspace/composer/index.tsx`
- `apps/agent-workspace/src/components/workspace/timeline-pane/index.tsx`
- `apps/agent-workspace/src/components/workspace/mind-panel/runtime-card.tsx`
- `apps/agent-workspace/src/lib/api/sessions.ts`
- `apps/agent-workspace/src/lib/api/normalizers.ts`
- `apps/agent-workspace/src-tauri/src/commands/sessions.rs`
- `apps/agent-workspace/src-tauri/src/kernel/composer/workspace.rs`
- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/store.rs`
- `apps/agent-workspace/src-tauri/src/kernel/live_timeline/snapshot.rs`
- `apps/agent-workspace/src-tauri/src/kernel/history/timeline.rs`
- `apps/agent-workspace/src-tauri/src/test_support/composer.rs`
- `apps/agent-workspace/src-tauri/tests/live_timeline_bridge.rs`
- `apps/agent-workspace/src-tauri/tests/workspace_composer.rs`
- `crates/agentic-kernel/src/prompt/agent_prompt.rs`
- `crates/agentic-kernel/src/commands/exec.rs`
- `crates/agentic-kernel/src/commands/process/turn_control.rs`
- `crates/agentic-kernel/src/runtime/output/assistant_turn_store.rs`
- `crates/agentic-kernel/src/runtime/output/token_path.rs`
- `crates/agentic-kernel/src/runtime/output/turn_completion.rs`
- `crates/agentic-kernel/src/events.rs`
- `crates/agentic-kernel/src/services/process_runtime.rs`
- `crates/agentic-kernel/src/test_support/process_commands.rs`
- `crates/agentic-kernel/src/test_support/e2e.rs`
- `crates/agentic-kernel/src/services/tests/process_runtime.rs`
- `crates/agentic-kernel/tests/e2e/process_commands.rs`
- `crates/agentic-kernel/tests/e2e/turn_boundaries.rs`

## Decisioni Architetturali

- Il filtro dei thinking vuoti resta bridge-side, non in GUI.
- Il projection layer continua a considerare veri solo gli eventi kernel-side strutturati; il testo assistant che imita output di sistema resta testo assistant.
- `STOP_OUTPUT` ha ora due semantiche:
  - hard stop immediato su `AwaitingTurnDecision`
  - soft stop request while in-flight, applicata alla prossima boundary sicura
- La GUI espone il controllo di stop direttamente nella composer bar, non dentro il flusso della timeline.
- Quando `cancel_generation` e' `false`, il bottone di stop resta semanticamente onesto e comunica il comportamento reale via tooltip/stato pending.
- Le quote configurabili in start session vengono trasportate nel payload `EXEC` come request JSON strutturata.
- L'override quota agisce sullo scheduler; il budget tecnico del singolo turno non viene dilatato artificialmente da un `No Limit`.
- Il bridge decide tra seed storico e append live del nuovo prompt; non combina piu' ciecamente entrambe le fonti quando il seed contiene gia' il turno appena persistito.

## Rischi / Regressioni

- Il payload `EXEC` ora supporta anche un request JSON strutturato; i client legacy plain-text restano supportati.
- `No Limit` sul quota scheduler viene rappresentato come `usize::MAX` kernel-side e normalizzato a `null` lato UI.
- Lo stop soft non cancella una request backend gia' partita: attende il completamento del passo corrente, perche' i backend attuali non espongono cancel provider-native.
- Nel package `agent-workspace` i nuovi test del workspace composer sono stati spostati fuori da `src/`; restano ancora altri test storici inline in moduli non toccati da questa iterazione.

## Strategia Di Validazione

- `cargo test -p agentic-kernel process_commands --tests`
- `cargo test -p agentic-kernel turn_boundaries --tests`
- `cargo test -p agentic-kernel agent_prompt --lib`
- `cargo test -p agentic-kernel process_runtime --lib`
- `cargo test -p agent-workspace --tests`
- `npm run build` in `apps/agent-workspace`
