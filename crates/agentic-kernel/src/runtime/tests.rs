use super::syscalls::scan_syscall_buffer;

#[test]
fn scan_finds_complete_action_command() {
    let mut buf = "some text\nACTION:spawn {\"prompt\":\"do something\"}".to_string();
    let result = scan_syscall_buffer(&mut buf);
    assert_eq!(
        result,
        Some("ACTION:spawn {\"prompt\":\"do something\"}".to_string())
    );
    assert!(buf.is_empty());
}

#[test]
fn scan_returns_none_for_plain_text() {
    let mut buf = "some text without any canonical prefix\n".to_string();
    let result = scan_syscall_buffer(&mut buf);
    assert!(result.is_none());
    assert!(!buf.is_empty());
}

#[test]
fn scan_clears_on_overflow() {
    let mut buf = "x".repeat(8001);
    let result = scan_syscall_buffer(&mut buf);
    assert!(result.is_none());
    assert!(buf.is_empty());
}

#[test]
fn scan_empty_buffer_returns_none() {
    let mut buf = String::new();
    let result = scan_syscall_buffer(&mut buf);
    assert!(result.is_none());
}

#[test]
fn scan_ignores_non_prefixed_content() {
    let mut buf = "random output with brackets [[ but no prefix".to_string();
    let result = scan_syscall_buffer(&mut buf);
    assert!(result.is_none());
    assert!(!buf.is_empty());
}

#[test]
fn scan_finds_complete_bare_tool_command() {
    let mut buf = "Prelude\nTOOL:python {\"code\":\"print(1)\"}".to_string();
    let result = scan_syscall_buffer(&mut buf);
    assert_eq!(
        result,
        Some("TOOL:python {\"code\":\"print(1)\"}".to_string())
    );
    assert!(buf.is_empty());
}

#[test]
fn scan_waits_for_complete_bare_tool_json() {
    let mut buf = "TOOL:python {\"code\":\"print(1)\"".to_string();
    let result = scan_syscall_buffer(&mut buf);
    assert!(result.is_none());
    assert!(!buf.is_empty());

    buf.push('}');
    let result = scan_syscall_buffer(&mut buf);
    assert_eq!(
        result,
        Some("TOOL:python {\"code\":\"print(1)\"}".to_string())
    );
    assert!(buf.is_empty());
}

#[test]
fn scan_waits_for_complete_action_json() {
    let mut buf = "ACTION:send {\"pid\":42".to_string();
    let result = scan_syscall_buffer(&mut buf);
    assert!(result.is_none());
    assert!(!buf.is_empty());

    buf.push_str(",\"message\":\"hello\"}");
    let result = scan_syscall_buffer(&mut buf);
    assert_eq!(
        result,
        Some("ACTION:send {\"pid\":42,\"message\":\"hello\"}".to_string())
    );
    assert!(buf.is_empty());
}

#[test]
fn scan_keeps_suffix_after_complete_tool_json_same_line() {
    let mut buf = "TOOL:python {\"code\":\"print(1)\"}La sequenza e' stata calcolata".to_string();
    let result = scan_syscall_buffer(&mut buf);
    assert_eq!(
        result,
        Some("TOOL:python {\"code\":\"print(1)\"}".to_string())
    );
    assert_eq!(buf, "La sequenza e' stata calcolata");
}

#[test]
fn scan_keeps_next_action_after_first_action_on_same_line() {
    let mut buf =
        "ACTION:spawn {\"prompt\":\"worker\"}ACTION:send {\"pid\":7,\"message\":\"ok\"}"
            .to_string();

    let first = scan_syscall_buffer(&mut buf);
    assert_eq!(
        first,
        Some("ACTION:spawn {\"prompt\":\"worker\"}".to_string())
    );
    assert_eq!(buf, "ACTION:send {\"pid\":7,\"message\":\"ok\"}");

    let second = scan_syscall_buffer(&mut buf);
    assert_eq!(
        second,
        Some("ACTION:send {\"pid\":7,\"message\":\"ok\"}".to_string())
    );
    assert!(buf.is_empty());
}
