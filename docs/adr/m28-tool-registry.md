# ADR M28 — Tool Registry Dinamico (contract freeze)

Data: 2026-03-09
Stato: Accepted

## Contesto

Il kernel esponeva tool/syscall tramite dispatch hardcoded su prefissi legacy (`PYTHON:`, `WRITE_FILE:`, `READ_FILE:`, `LS`, `CALC:`), con discovery non strutturata e `TOOL_INFO` statico.

M28 introduce un registry dinamico, interrogabile dal control plane e riusabile dal runtime.

## Decisioni

1. Esiste una forma canonica unica di invocazione tool nel runtime:

```text
[[TOOL:<name> <json-object>]]
```

Esempi:

```text
[[TOOL:python {"code":"print(1)"}]]
[[TOOL:read_file {"path":"src/main.rs"}]]
[[TOOL:list_files {}]]
```

2. I prefissi legacy restano supportati solo come alias/adapter verso il formato canonico:

- `PYTHON:` -> `python`
- `WRITE_FILE:` -> `write_file`
- `READ_FILE:` -> `read_file`
- `LS` -> `list_files`
- `CALC:` -> `calc`

3. Il registry separa metadati e backend operativo:

- `ToolDescriptor`: nome canonico, alias, descrizione, schema input, schema output, backend kind, capabilities, flags.
- `ToolBackendConfig`: dettagli esecutivi specifici del backend. Nel primo slice e' implicito per i built-in; i backend remoti verranno introdotti nello slice successivo.
- `ToolRegistryEntry`: descriptor + provenance/runtime flags.

4. `LIST_TOOLS` e' l'indice completo dei tool registrati.

5. `TOOL_INFO <name>` e' una query puntuale per singolo tool, non un dump globale.

6. Il dispatch runtime consulta il registry prima di eseguire il tool. Gli alias legacy vengono tradotti prima dell'esecuzione.

## Contratto dati minimo

`ToolDescriptor` campi minimi:

- `name`
- `aliases`
- `description`
- `input_schema`
- `output_schema`
- `backend_kind`
- `capabilities`
- `dangerous`
- `enabled`
- `source`

## Motivazioni

- Un solo formato canonico evita drift tra control-plane e runtime.
- Gli alias legacy preservano compatibilita' senza perpetuare branch hardcoded fuori dal registry.
- La separazione descriptor/backend evita di contaminare il catalogo con dettagli esecutivi non serializzabili.

## Conseguenze

- `REGISTER_TOOL` / `UNREGISTER_TOOL` arriveranno nello slice successivo e dovranno essere privilegiati.
- Il backend remoto HTTP verra' aggiunto successivamente, con allowlist, timeout hard e payload limits.
- Nel primo rilascio il registry resta in-memory e non viene checkpointato.