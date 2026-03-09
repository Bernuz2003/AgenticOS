from __future__ import annotations

import json
import unittest

from gui.response_parser import normalize_control_payload, parse_json_dict, parse_protocol_envelope


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


if __name__ == "__main__":
    unittest.main()
