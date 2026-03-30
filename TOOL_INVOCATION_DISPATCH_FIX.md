# Tool Invocation Dispatch Fix

## Sintomo verificato

- La GUI riconosceva la tool call e mostrava il box dedicato.
- I processi locali chiudevano il turno in `WaitingForInput` senza dispatch.
- I processi remoti potevano continuare l'inferenza dopo la tool call oppure reiterarla.
- Negli audit reali non compariva `tool/dispatched`.

## Evidenza raccolta

Ho aggiunto e usato due test live contro Groq:

- `live_groq_stream_dump_for_tool_invocation`
- `live_kernel_groq_dispatches_tool_invocation`

Il dump reale dei chunk ha mostrato che Groq spezza spesso il marker su piu' delta:

- `"TO"`
- `"OL"`
- `":"`
- `"mkdir"`

Quindi il problema non era solo "riconoscere `TOOL:` quando c'e' tutto", ma anche non perdere il marker mentre arriva frammentato.

## Causa reale

Il bug era composto da due difetti concorrenti nella pipeline kernel-side.

### 1. Riconoscimento troppo ristretto

Backend e runtime riconoscevano la syscall soprattutto quando il marker era gia' completo e, in pratica, a inizio riga.

Questo faceva fallire casi validi come:

- testo introduttivo prima della syscall;
- menzioni testuali innocue come `funzione TOOL:mkdir.`;
- syscall canonica valida presente piu' avanti nello stesso output.

### 2. Perdita dei marker parziali nello stream

Nel path `StreamChunk`, il runtime svuotava immediatamente frammenti come `TO`, `OL`, `:` come testo normale.

Con modelli remoti che streammano il marker spezzato:

1. il backend remoto fermava correttamente la risposta sul boundary della syscall;
2. ma il runtime aveva gia' disperso i pezzi del marker;
3. quindi `pending_stream_syscall` restava vuoto;
4. il `Token` finale non aveva piu' nulla da dispatchare;
5. il processo veniva ri-checkoutato o finiva in `WaitingForInput`.

Questo era il motivo subdolo per cui il test live falliva anche quando `step.emitted_text` era gia' troncato correttamente a `TOOL:mkdir {...}`.

## Fix applicato

### Parser condiviso

In `crates/agentic-kernel/src/invocation/text.rs` ho introdotto una ricerca canonica della prima invocation valida:

- trova `TOOL:` e `ACTION:` ovunque nel testo;
- distingue tra `Parsed`, `Incomplete` e `NotFound`;
- ignora menzioni invalide prima di una vera invocation successiva.

### Backend locali/remoti

In `crates/agentic-kernel/src/backend/remote/streaming.rs` `agent_invocation_end()` usa ora la stessa semantica canonica del parser condiviso.

Questo riallinea il boundary detector del backend al dispatcher del kernel.

### Runtime stream/token path

In `crates/agentic-kernel/src/runtime/output/assistant_output.rs` il runtime ora:

- intercetta syscall valide anche inline, non solo a inizio riga;
- conserva i prefissi parziali di `TOOL:` / `ACTION:` tra chunk consecutivi;
- continua a trattare come errore solo le invocation malformate esplicite a inizio riga.

### Scanner di fallback

In `crates/agentic-kernel/src/runtime/syscalls/dispatch.rs` lo scanner syscall riconosce ora anche invocation valide non allineate all'inizio riga, mantenendo il fallback storico per i comandi malformati espliciti.

## Piano operativo eseguito

1. Riproduzione reale del bug contro Groq.
2. Verifica dei chunk streamati e del boundary effettivo della tool call.
3. Unificazione della semantica parser/backend/runtime.
4. Gestione corretta dei marker parziali attraverso i delta streamati.
5. Verifica end-to-end con dispatch reale del tool nel kernel.

## Test aggiunti o rinforzati

- ricerca della prima invocation valida inline;
- ignorare una menzione invalida prima di una vera tool call;
- buffering di `TOOL:` spezzato su piu' chunk;
- dispatch end-to-end da stream chunk con marker spezzato;
- truncation remoto inline nel decoder OpenAI-compatible;
- test live Groq del backend;
- test live Groq del kernel con dispatch reale.

## Esito finale

Il test live end-to-end ora passa:

- la syscall viene intercettata;
- il processo entra in `WaitingForSyscall`;
- compare l'audit di dispatch;
- il tool `mkdir` viene realmente eseguito nel workspace;
- l'output del tool rientra nel flusso del processo.
