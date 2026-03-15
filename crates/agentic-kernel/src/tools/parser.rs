use crate::tools::error::ToolError;
use crate::tools::invocation::ToolInvocation;

/// Parsa una stringa testuale stretta nel formato:
/// `TOOL:<name> <json>`
/// Rimuove spazi iniziali/finali ma esige che la riga inizi esattamente con `TOOL:`.
pub fn parse_text_invocation(text: &str) -> Result<ToolInvocation, ToolError> {
    let (name, input) = crate::text_invocation::parse_prefixed_json_invocation(text, "TOOL:")
        .map_err(ToolError::MalformedInvocation)?;
    crate::tools::executor::build_structured_invocation(name, input, None)
}

/// Controlla se il testo corrente che inizia per "TOOL:" sta ancora streammando.
/// Ritorna `true` se siamo in presenza di JSON incompleto ma non corrotto (EOF parser error).
pub fn is_streaming_tool_invocation(text: &str) -> bool {
    crate::text_invocation::is_streaming_prefixed_json_invocation(text, "TOOL:")
}

#[cfg(test)]
mod tests {
    use super::{is_streaming_tool_invocation, parse_text_invocation};

    #[test]
    fn parses_canonical_tool_invocation() {
        let parsed = parse_text_invocation(r#"TOOL:read_file {"path":"notes/todo.md"}"#)
            .expect("tool parse");
        assert_eq!(parsed.name, "read_file");
        assert_eq!(parsed.input["path"], "notes/todo.md");
    }

    #[test]
    fn rejects_non_object_payload() {
        let err = parse_text_invocation(r#"TOOL:calc ["1+1"]"#).expect_err("payload rejection");
        assert!(err.to_string().contains("JSON object"));
    }

    #[test]
    fn rejects_non_canonical_tool_name() {
        let err = parse_text_invocation(r#"TOOL:PYTHON {"code":"print(1)"}"#)
            .expect_err("non canonical name");
        assert!(err.to_string().contains("not canonical"));
    }

    #[test]
    fn detects_streaming_json_payload() {
        assert!(is_streaming_tool_invocation(
            r#"TOOL:calc {"expression":"1+1""#
        ));
    }
}
