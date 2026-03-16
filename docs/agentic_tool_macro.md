# `#[agentic_tool]` Macro MVP

## Obiettivo

`#[agentic_tool]` riduce il boilerplate dei built-in Rust semplici nel kernel senza cambiare il contratto del tool system.

Nel MVP la macro resta volutamente piccola:

- descrive un tool host built-in contract-first
- genera schema input/output da tipi Rust
- genera l'adapter verso `Tool`
- genera `ToolRegistryEntry`
- genera il binding host built-in per il catalogo esplicito del kernel

La macro non conosce:

- sintassi `TOOL:<name> <json-object>`
- sintassi `ACTION:<name> <json-object>`
- parser testuale
- governance, audit o rate limiting
- dispatcher remoto o backend provider-specific

Questa separazione mantiene il design compatibile con Programmatic Tool Calling: il parser testuale e gli ingressi strutturati convergono sulla stessa `ToolInvocation`, mentre la macro produce solo glue contrattuale.

## Uso

```rust
use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadFileInput {
    path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct ReadFileOutput {
    output: String,
    path: String,
}

#[agentic_tool(
    name = "read_file",
    description = "Read a UTF-8 text file inside the workspace root.",
    capabilities = ["fs", "read"],
    allowed_callers = [AgentText, Programmatic]
)]
fn read_file(input: ReadFileInput, ctx: &ToolContext) -> Result<ReadFileOutput, ToolError> {
    // business logic
}
```

## Cosa genera

Per una funzione `fn read_file(...) -> Result<Output, ToolError>`, il MVP genera:

- uno unit struct tool (`ReadFileTool`)
- `impl Tool for ReadFileTool`
- `read_file_registry_entry() -> ToolRegistryEntry`
- `read_file_host_builtin_registration() -> HostBuiltinRegistration`

Il wrapper runtime fa:

1. `serde_json::from_value(invocation.input)` verso l'input typed
2. chiamata alla funzione Rust annotata
3. conversione dell'output verso `ToolResult`

### Dual return type

La macro supporta due forme di ritorno:

#### `Result<Output, ToolError>` (typed output)

L'output viene serializzato automaticamente a JSON. La policy per `display_text`:
- se l'output serializzato contiene un campo top-level stringa `output`, quel valore diventa `display_text`
- altrimenti `display_text` usa il JSON pretty-printed
- `warnings` resta vuoto

Questo preserva il comportamento dei built-in semplici (`read_file`, `list_files`, `calc`).

L'output schema viene derivato automaticamente dal tipo Rust.

#### `Result<ToolResult, ToolError>` (passthrough)

Il tool ha pieno controllo su `output`, `display_text` e `warnings`.
Il wrapper non serializza ne' trasforma nulla: fa solo passthrough.

Per mantenere descriptor e schema leggibili, il MVP supporta anche un hint esplicito:

```rust
#[agentic_tool(
    name = "custom_tool",
    description = "Custom display_text rendering",
    allowed_callers = [AgentText, Programmatic],
    output_schema_type = CustomToolOutput
)]
fn custom_tool(...) -> Result<ToolResult, ToolError> {
    // ...
}
```

Se `output_schema_type` e' presente, il registry usa lo schema del tipo indicato.
Se l'opzione manca, l'output schema resta permissivo (`{ "type": "object" }`) perche' il tool gestisce la struttura del risultato direttamente.

Questo e' utile per tool che:
- hanno un `display_text` che non corrisponde direttamente all'output strutturato
- generano warnings non bloccanti
- necessitano di formattazione custom per l'agente/UI

## Registrazione nel kernel

La registrazione resta esplicita e reviewable.

Il kernel usa un catalogo host built-in centralizzato in [`crates/agentic-kernel/src/tools/builtins.rs`](/home/bernuz/Progetti/AgenticOS/agenticOS/crates/agentic-kernel/src/tools/builtins.rs), che e' l'unica source of truth condivisa tra:

- [`ToolRegistry::with_builtins()`](/home/bernuz/Progetti/AgenticOS/agenticOS/crates/agentic-kernel/src/tool_registry.rs)
- [`ToolDispatcher::new()`](/home/bernuz/Progetti/AgenticOS/agenticOS/crates/agentic-kernel/src/tools/dispatcher.rs)

I built-in macro-generated usano `HostExecutor::Dynamic(String)` come binding host esplicito. La serializzazione resta una semplice stringa nel backend JSON, quindi i contratti `LIST_TOOLS` e `TOOL_INFO` non diventano piu' opachi.

## Limiti intenzionali del MVP

Il MVP supporta solo:

- free function non-async
- firma `fn(input, ctx: &ToolContext) -> Result<Output, ToolError>` (typed output)
- firma `fn(input, ctx: &ToolContext) -> Result<ToolResult, ToolError>` (passthrough)
- input typed by-value
- tool host built-in Rust

Il MVP non supporta ancora:

- action plane
- remote HTTP tools
- wasm tools
- auto-registration globale implicita
- metadata avanzati oltre a nome, descrizione, capabilities, allowed callers, dangerous, enabled, aliases

## Comportamento di errore

La macro prova a fallire in modo reviewable:

- firme non supportate, tool name non canonici e `allowed_callers = []` producono errori di compilazione leggibili
- la generazione schema a runtime non fa panic al boot: logga l'errore e ripiega su uno schema permissivo
- `output_schema_type` e' accettato solo per tool `Result<ToolResult, ToolError>`, cosi' il binding resta esplicito

## Compatibilita' con PTC

La macro non dipende dal parser testuale e non genera logica di transport.

Il flusso resta:

1. il transport costruisce o parse-a una `ToolInvocation`
2. governance applica policy, audit e rate limiting
3. il dispatcher risolve il backend host e invoca `Tool`
4. il tool macro-generated esegue solo il glue typed

Quindi lo stesso built-in puo' essere invocato sia dal text path sia da ingressi structured/PTC-oriented futuri, senza cambiare la macro.
