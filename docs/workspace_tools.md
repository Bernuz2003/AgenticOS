# Workspace Tools

## Obiettivo

Questi built-in estendono il tool plane con primitive locali ad alto leverage per orientamento, ricerca e lettura selettiva nel workspace.

Restano tutti:

- local-first
- host built-in
- compatibili con text invocation, structured invocation e futuri ingressi PTC
- separati da parser, governance e action plane

## Tool disponibili

### `path_info`

Legge metadata essenziali di un file o directory nel workspace.

- Input: `path`
- Output: `exists`, `entry_type`, `size_bytes`, `modified_unix_ms`
- Caso utile: controllare se un path esiste prima di leggerlo o scriverlo

Esempio text path:

```text
TOOL:path_info {"path":"src/main.rs"}
```

### `find_files`

Cerca file per nome e/o estensione sotto una root del workspace.

- Input principali: `path`, `pattern`, `extension`, `recursive`, `max_results`
- Regola MVP: serve almeno uno tra `pattern` e `extension`
- Output: `matches`, `root`, `truncated`

Esempio text path:

```text
TOOL:find_files {"path":"crates","pattern":"tool","extension":"rs","recursive":true,"max_results":20}
```

### `search_text`

Cerca testo nei file UTF-8 del workspace.

- Input principali: `query`, `path`, `recursive`, `case_sensitive`, `max_results`
- Output: lista di match con `path`, `line`, `column`, `text`
- Limite MVP: salta file non UTF-8 o oltre il limite di dimensione previsto dal tool

Esempio text path:

```text
TOOL:search_text {"query":"ToolRegistry","path":"crates/agentic-kernel/src","recursive":true,"max_results":20}
```

### `read_file_range`

Legge solo un intervallo inclusivo di righe da un file UTF-8.

- Input: `path`, `start_line`, `end_line`
- Output: `lines` numerate e `display_text` pronto per l'agente
- Limite MVP: massimo 200 righe per chiamata

Esempio text path:

```text
TOOL:read_file_range {"path":"crates/agentic-kernel/src/tool_registry.rs","start_line":1,"end_line":80}
```

### `mkdir`

Crea directory nel workspace in modo sicuro.

- Input: `path`, `create_parents`
- Output: `created`
- Comportamento: se la directory esiste gia', il tool risponde con successo e `created = false`

Esempio text path:

```text
TOOL:mkdir {"path":"workspace/tmp/reports","create_parents":true}
```

## Sicurezza e limiti

- tutti i path passano dal path guard del kernel e non possono uscire dal workspace
- `search_text` e `read_file_range` leggono solo file UTF-8 entro i limiti del tool
- `find_files` e `search_text` hanno `max_results` clampato per evitare output eccessivi
- nessuno di questi tool introduce esecuzione shell, networking o cancellazioni distruttive
