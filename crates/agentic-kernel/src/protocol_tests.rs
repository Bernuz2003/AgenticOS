use agentic_protocol::MAX_CONTENT_LENGTH;

    use super::{
        handle_hello, response_err_code, response_ok_code, stable_capabilities, CommandHeader,
        OpCode,
    };
    use crate::transport::Client;

    fn test_client() -> Client {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let join = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept client");
            stream
        });

        let client_stream = std::net::TcpStream::connect(addr).expect("connect listener");
        let _server_stream = join.join().expect("join accept thread");
        Client::new(mio::net::TcpStream::from_std(client_stream), true)
    }

    #[test]
    fn parse_basic_opcodes() {
        let hello = CommandHeader::parse("HELLO 1 0").expect("HELLO should parse");
        assert!(matches!(hello.opcode, OpCode::Hello));

        let ping = CommandHeader::parse("PING 1 0").expect("PING should parse");
        assert!(matches!(ping.opcode, OpCode::Ping));

        let load = CommandHeader::parse("LOAD 1 10").expect("LOAD should parse");
        assert!(matches!(load.opcode, OpCode::Load));

        let status = CommandHeader::parse("STATUS 1 0").expect("STATUS should parse");
        assert!(matches!(status.opcode, OpCode::Status));

        let register_tool =
            CommandHeader::parse("REGISTER_TOOL 1 2").expect("REGISTER_TOOL should parse");
        assert!(matches!(register_tool.opcode, OpCode::RegisterTool));

        let unregister_tool =
            CommandHeader::parse("UNREGISTER_TOOL 1 2").expect("UNREGISTER_TOOL should parse");
        assert!(matches!(unregister_tool.opcode, OpCode::UnregisterTool));

        let term = CommandHeader::parse("TERM 1 1").expect("TERM should parse");
        assert!(matches!(term.opcode, OpCode::Term));

        let kill = CommandHeader::parse("KILL 1 1").expect("KILL should parse");
        assert!(matches!(kill.opcode, OpCode::Kill));

        let shutdown = CommandHeader::parse("SHUTDOWN 1 0").expect("SHUTDOWN should parse");
        assert!(matches!(shutdown.opcode, OpCode::Shutdown));

        let continue_output =
            CommandHeader::parse("CONTINUE_OUTPUT 1 9").expect("CONTINUE_OUTPUT should parse");
        assert!(matches!(continue_output.opcode, OpCode::ContinueOutput));

        let stop_output =
            CommandHeader::parse("STOP_OUTPUT 1 9").expect("STOP_OUTPUT should parse");
        assert!(matches!(stop_output.opcode, OpCode::StopOutput));
    }

    #[test]
    fn parse_extended_model_opcodes() {
        let list = CommandHeader::parse("LIST_MODELS 1 0").expect("LIST_MODELS should parse");
        assert!(matches!(list.opcode, OpCode::ListModels));

        let select = CommandHeader::parse("SELECT_MODEL 1 7").expect("SELECT_MODEL should parse");
        assert!(matches!(select.opcode, OpCode::SelectModel));

        let info = CommandHeader::parse("MODEL_INFO 1 0").expect("MODEL_INFO should parse");
        assert!(matches!(info.opcode, OpCode::ModelInfo));

        let diag = CommandHeader::parse("BACKEND_DIAG 1 0").expect("BACKEND_DIAG should parse");
        assert!(matches!(diag.opcode, OpCode::BackendDiag));

        let set_gen = CommandHeader::parse("SET_GEN 1 10").expect("SET_GEN should parse");
        assert!(matches!(set_gen.opcode, OpCode::SetGen));

        let get_gen = CommandHeader::parse("GET_GEN 1 0").expect("GET_GEN should parse");
        assert!(matches!(get_gen.opcode, OpCode::GetGen));

        let send_input = CommandHeader::parse("SEND_INPUT 1 32").expect("SEND_INPUT should parse");
        assert!(matches!(send_input.opcode, OpCode::SendInput));
    }

    #[test]
    fn parse_scheduler_opcodes() {
        let sp = CommandHeader::parse("SET_PRIORITY 1 5").expect("SET_PRIORITY should parse");
        assert!(matches!(sp.opcode, OpCode::SetPriority));

        let gq = CommandHeader::parse("GET_QUOTA 1 2").expect("GET_QUOTA should parse");
        assert!(matches!(gq.opcode, OpCode::GetQuota));

        let sq = CommandHeader::parse("SET_QUOTA 1 20").expect("SET_QUOTA should parse");
        assert!(matches!(sq.opcode, OpCode::SetQuota));
    }

    #[test]
    fn parse_checkpoint_opcodes() {
        let cp = CommandHeader::parse("CHECKPOINT 1 0").expect("CHECKPOINT should parse");
        assert!(matches!(cp.opcode, OpCode::Checkpoint));

        let rs = CommandHeader::parse("RESTORE 1 0").expect("RESTORE should parse");
        assert!(matches!(rs.opcode, OpCode::Restore));
    }

    #[test]
    fn parse_orchestrate_opcode() {
        let o = CommandHeader::parse("ORCHESTRATE agent_1 200").expect("ORCHESTRATE should parse");
        assert!(matches!(o.opcode, OpCode::Orchestrate));
        assert_eq!(o.agent_id, "agent_1");
        assert_eq!(o.content_length, 200);

        let list_tools = CommandHeader::parse("LIST_TOOLS 1 0").expect("LIST_TOOLS should parse");
        assert!(matches!(list_tools.opcode, OpCode::ListTools));

        let tool_info = CommandHeader::parse("TOOL_INFO 1 0").expect("TOOL_INFO should parse");
        assert!(matches!(tool_info.opcode, OpCode::ToolInfo));
    }

    #[test]
    fn parse_memw_case_insensitive() {
        let memw = CommandHeader::parse("memw 1 4").expect("memw should parse");
        assert!(matches!(memw.opcode, OpCode::MemoryWrite));
    }

    #[test]
    fn parse_invalid_opcode() {
        let err = CommandHeader::parse("WHAT 1 0").expect_err("invalid opcode must fail");
        assert!(err.to_string().contains("Unknown opcode"));
    }

    #[test]
    fn parse_requires_three_tokens() {
        let err = CommandHeader::parse("PING").expect_err("header without fields must fail");
        assert!(err.to_string().contains("Invalid header format"));

        let err =
            CommandHeader::parse("PING 1 0 extra").expect_err("header with extra fields fails");
        assert!(err.to_string().contains("Invalid header format"));
    }

    #[test]
    fn parse_rejects_oversized_payloads() {
        let err = CommandHeader::parse(&format!("EXEC 1 {}", MAX_CONTENT_LENGTH + 1))
            .expect_err("oversized payload must fail");
        assert!(err.to_string().contains("exceeds protocol limit"));
    }

    #[test]
    fn coded_response_format() {
        let ok = String::from_utf8(response_ok_code("PING", "PONG")).expect("utf8 ok");
        assert!(ok.starts_with("+OK PING 4\r\n"));
        assert!(ok.ends_with("PONG"));

        let err = String::from_utf8(response_err_code("BAD_HEADER", "Malformed")).expect("utf8 ok");
        assert!(err.starts_with("-ERR BAD_HEADER 9\r\n"));
        assert!(err.ends_with("Malformed"));
    }

    #[test]
    fn hello_negotiates_protocol_v1() {
        let mut client = test_client();
        let response = String::from_utf8(handle_hello(
            &mut client,
            br#"{"supported_versions":["v1"],"required_capabilities":[]}"#,
            "req-1",
        ))
        .expect("utf8 response");

        assert!(response.starts_with("+OK HELLO "));
        assert!(response.contains("\"protocol_version\":\"v1\""));
        assert_eq!(client.negotiated_protocol_version.as_deref(), Some("v1"));
    }

    #[test]
    fn stable_capabilities_are_non_empty() {
        assert!(!stable_capabilities().is_empty());
    }
