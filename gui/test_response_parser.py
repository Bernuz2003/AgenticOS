from __future__ import annotations

import json
import unittest

from gui.response_parser import (
    normalize_control_payload,
    parse_json_dict,
    parse_json_payload,
    parse_models_payload,
    parse_protocol_envelope,
    parse_tool_info_payload,
    parse_tools_payload,
)


class ResponseParserContractTests(unittest.TestCase):
    def test_parse_protocol_envelope_recognizes_v1_shape(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.status.v1",
                "request_id": "1:3",
                "ok": True,
                "code": "STATUS",
                "data": {"uptime_secs": 1},
                "error": None,
                "warnings": [],
            }
        )
        envelope = parse_protocol_envelope(payload)
        self.assertIsNotNone(envelope)
        assert envelope is not None
        self.assertEqual(envelope["schema_id"], "agenticos.control.status.v1")

    def test_normalize_control_payload_unwraps_success_data(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.tool_info.v1",
                "request_id": "1:4",
                "ok": True,
                "code": "TOOL_INFO",
                "data": {"tools": [{"id": "LS", "description": "List workspace files"}]},
                "error": None,
                "warnings": [],
            }
        )
        normalized = normalize_control_payload(payload, True)
        self.assertEqual(json.loads(normalized)["tools"][0]["id"], "LS")

    def test_parse_json_dict_unwraps_envelope_data(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.get_quota.v1",
                "request_id": "1:5",
                "ok": True,
                "code": "GET_QUOTA",
                "data": {"pid": 77, "priority": "high"},
                "error": None,
                "warnings": [],
            }
        )
        self.assertEqual(parse_json_dict(payload), {"pid": 77, "priority": "high"})

    def test_parse_json_payload_unwraps_list_data(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.list_models.v1",
                "request_id": "1:7",
                "ok": True,
                "code": "LIST_MODELS",
                "data": [{"id": "llama3.1-8b", "family": "Llama"}],
                "error": None,
                "warnings": [],
            }
        )
        self.assertEqual(parse_json_payload(payload), [{"id": "llama3.1-8b", "family": "Llama"}])

    def test_parse_models_payload_accepts_enveloped_array(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.list_models.v1",
                "request_id": "1:8",
                "ok": True,
                "code": "LIST_MODELS",
                "data": [{"id": "llama3.1-8b", "family": "Llama", "path": "/models/llama.gguf"}],
                "error": None,
                "warnings": [],
            }
        )
        models, routing = parse_models_payload(payload)
        self.assertEqual(len(models), 1)
        self.assertEqual(models[0]["id"], "llama3.1-8b")
        self.assertEqual(routing, {})

    def test_parse_models_payload_accepts_enveloped_object(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.list_models.v1",
                "request_id": "1:9",
                "ok": True,
                "code": "LIST_MODELS",
                "data": {
                    "models": [{"id": "qwen2.5-14b", "family": "Qwen", "path": "/models/qwen.gguf"}],
                    "routing_recommendations": [{"workload": "code", "model_id": "qwen2.5-14b"}],
                },
                "error": None,
                "warnings": [],
            }
        )
        models, routing = parse_models_payload(payload)
        self.assertEqual(models[0]["id"], "qwen2.5-14b")
        self.assertEqual(routing["code"]["model_id"], "qwen2.5-14b")

    def test_normalize_control_payload_extracts_error_message(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.error.v1",
                "request_id": "1:6",
                "ok": False,
                "code": "PID_NOT_FOUND",
                "data": None,
                "error": {"message": "PID 99 not found"},
                "warnings": [],
            }
        )
        self.assertEqual(normalize_control_payload(payload, False), "PID 99 not found")

    def test_parse_tools_payload_unwraps_registry_entries(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.list_tools.v1",
                "request_id": "1:10",
                "ok": True,
                "code": "LIST_TOOLS",
                "data": {
                    "total_tools": 1,
                    "tools": [
                        {
                            "descriptor": {
                                "name": "python",
                                "aliases": ["PYTHON"],
                                "description": "Execute Python code.",
                                "input_schema": {"type": "object"},
                                "output_schema": {"type": "object"},
                                "backend_kind": "host",
                                "capabilities": ["python"],
                                "dangerous": True,
                                "enabled": True,
                                "source": "built_in",
                            },
                            "backend": {"kind": "host", "executor": "builtin_python"},
                        }
                    ],
                },
                "error": None,
                "warnings": [],
            }
        )

        tools = parse_tools_payload(payload)
        self.assertEqual(len(tools), 1)
        self.assertEqual(tools[0]["name"], "python")
        self.assertEqual(tools[0]["backend_kind"], "host")
        self.assertEqual(tools[0]["aliases"], ["PYTHON"])

    def test_parse_tool_info_payload_unwraps_tool_and_sandbox(self) -> None:
        payload = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.tool_info.v1",
                "request_id": "1:11",
                "ok": True,
                "code": "TOOL_INFO",
                "data": {
                    "tool": {
                        "descriptor": {
                            "name": "remote_echo",
                            "aliases": [],
                            "description": "Forward payload.",
                            "input_schema": {"type": "object"},
                            "output_schema": {"type": "object"},
                            "backend_kind": "remote_http",
                            "capabilities": ["remote"],
                            "dangerous": False,
                            "enabled": True,
                            "source": "runtime",
                        },
                        "backend": {
                            "kind": "remote_http",
                            "url": "http://127.0.0.1:8081/invoke",
                            "method": "POST",
                            "timeout_ms": 1000,
                            "headers": {},
                        },
                    },
                    "sandbox": {"mode": "host", "timeout_s": 8},
                },
                "error": None,
                "warnings": [],
            }
        )

        info = parse_tool_info_payload(payload)
        self.assertEqual(info["tool"]["name"], "remote_echo")
        self.assertEqual(info["tool"]["backend"]["kind"], "remote_http")
        self.assertEqual(info["sandbox"]["timeout_s"], 8)


if __name__ == "__main__":
    unittest.main()
