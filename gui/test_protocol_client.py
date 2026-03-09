from __future__ import annotations

import json
import unittest
from unittest.mock import patch

from gui.protocol_client import ProtocolClient, consume_framed_messages


class FakeSocket:
    def __init__(self, recv_chunks: list[bytes]):
        self._recv_chunks = list(recv_chunks)
        self.sent = bytearray()
        self.timeout = None
        self.connected_to = None
        self.closed = False

    def settimeout(self, value):
        self.timeout = value

    def connect(self, addr):
        self.connected_to = addr

    def sendall(self, data: bytes):
        self.sent.extend(data)

    def recv(self, _size: int) -> bytes:
        if self._recv_chunks:
            return self._recv_chunks.pop(0)
        return b""

    def close(self):
        self.closed = True


class ProtocolClientContractTests(unittest.TestCase):
    def test_consume_framed_messages_parses_control_frame(self) -> None:
        body = b'{"ok":true}'
        buf = bytearray(f'+OK STATUS {len(body)}\r\n'.encode("utf-8") + body)
        frames = consume_framed_messages(buf)
        self.assertEqual(len(frames), 1)
        self.assertEqual(frames[0][0], "+OK")
        self.assertEqual(frames[0][1], "STATUS")
        self.assertEqual(frames[0][2], body)

    @patch("gui.protocol_client.load_runtime_defaults")
    def test_negotiate_protocol_tracks_version_and_capabilities(self, load_defaults) -> None:
        load_defaults.return_value = {
            "host": "127.0.0.1",
            "port": 6380,
            "kernel_token_path": "workspace/.kernel_token",
        }
        hello_body = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.hello.v1",
                "request_id": "1:1",
                "ok": True,
                "code": "HELLO",
                "data": {
                    "negotiated_version": "v1",
                    "enabled_capabilities": ["control_envelope_v1", "tool_info_v1"],
                    "legacy_fallback_allowed": True,
                },
                "error": None,
                "warnings": [],
            }
        ).encode("utf-8")
        fake = FakeSocket([f'+OK HELLO {len(hello_body)}\r\n'.encode("utf-8") + hello_body])

        client = ProtocolClient()
        client._negotiate_protocol(fake)

        sent_text = fake.sent.decode("utf-8", errors="replace")
        self.assertIn("HELLO 1 ", sent_text)
        self.assertEqual(client._negotiated_version, "v1")
        self.assertIn("tool_info_v1", client._enabled_capabilities)

    @patch("gui.protocol_client.load_runtime_defaults")
    def test_send_once_unwraps_v1_envelope_payload(self, load_defaults) -> None:
        load_defaults.return_value = {
            "host": "127.0.0.1",
            "port": 6380,
            "kernel_token_path": "workspace/.kernel_token",
        }
        hello_body = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.hello.v1",
                "request_id": "1:1",
                "ok": True,
                "code": "HELLO",
                "data": {
                    "negotiated_version": "v1",
                    "enabled_capabilities": ["control_envelope_v1"],
                    "legacy_fallback_allowed": True,
                },
                "error": None,
                "warnings": [],
            }
        ).encode("utf-8")
        status_body = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.status.v1",
                "request_id": "1:2",
                "ok": True,
                "code": "STATUS",
                "data": {"uptime_secs": 42, "total_commands": 7},
                "error": None,
                "warnings": [],
            }
        ).encode("utf-8")
        fake = FakeSocket([
            f'+OK HELLO {len(hello_body)}\r\n'.encode("utf-8") + hello_body,
            f'+OK STATUS {len(status_body)}\r\n'.encode("utf-8") + status_body,
        ])

        client = ProtocolClient()
        client._authenticate = lambda sock: None
        client._sock = None

        with patch("gui.protocol_client.socket.socket", return_value=fake):
            response = client.send_once("STATUS", "")

        self.assertTrue(response.ok)
        self.assertEqual(response.code, "STATUS")
        self.assertEqual(json.loads(response.payload)["uptime_secs"], 42)

    @patch("gui.protocol_client.load_runtime_defaults")
    def test_send_once_unwraps_list_tools_payload(self, load_defaults) -> None:
        load_defaults.return_value = {
            "host": "127.0.0.1",
            "port": 6380,
            "kernel_token_path": "workspace/.kernel_token",
        }
        hello_body = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.hello.v1",
                "request_id": "1:1",
                "ok": True,
                "code": "HELLO",
                "data": {
                    "negotiated_version": "v1",
                    "enabled_capabilities": ["control_envelope_v1", "tool_registry_v1", "list_tools_v1"],
                    "legacy_fallback_allowed": True,
                },
                "error": None,
                "warnings": [],
            }
        ).encode("utf-8")
        list_tools_body = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.list_tools.v1",
                "request_id": "1:2",
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
        ).encode("utf-8")
        fake = FakeSocket([
            f'+OK HELLO {len(hello_body)}\r\n'.encode("utf-8") + hello_body,
            f'+OK LIST_TOOLS {len(list_tools_body)}\r\n'.encode("utf-8") + list_tools_body,
        ])

        client = ProtocolClient()
        client._authenticate = lambda sock: None
        client._sock = None

        with patch("gui.protocol_client.socket.socket", return_value=fake):
            response = client.send_once("LIST_TOOLS", "")

        self.assertTrue(response.ok)
        payload = json.loads(response.payload)
        self.assertEqual(payload["total_tools"], 1)
        self.assertEqual(payload["tools"][0]["descriptor"]["name"], "python")

    @patch("gui.protocol_client.load_runtime_defaults")
    def test_send_once_unwraps_tool_info_payload(self, load_defaults) -> None:
        load_defaults.return_value = {
            "host": "127.0.0.1",
            "port": 6380,
            "kernel_token_path": "workspace/.kernel_token",
        }
        hello_body = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.hello.v1",
                "request_id": "1:1",
                "ok": True,
                "code": "HELLO",
                "data": {
                    "negotiated_version": "v1",
                    "enabled_capabilities": ["control_envelope_v1", "tool_info_v1"],
                    "legacy_fallback_allowed": True,
                },
                "error": None,
                "warnings": [],
            }
        ).encode("utf-8")
        tool_info_body = json.dumps(
            {
                "protocol_version": "v1",
                "schema_id": "agenticos.control.tool_info.v1",
                "request_id": "1:2",
                "ok": True,
                "code": "TOOL_INFO",
                "data": {
                    "tool": {
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
                    },
                    "sandbox": {"mode": "host", "timeout_s": 8},
                },
                "error": None,
                "warnings": [],
            }
        ).encode("utf-8")
        fake = FakeSocket([
            f'+OK HELLO {len(hello_body)}\r\n'.encode("utf-8") + hello_body,
            f'+OK TOOL_INFO {len(tool_info_body)}\r\n'.encode("utf-8") + tool_info_body,
        ])

        client = ProtocolClient()
        client._authenticate = lambda sock: None
        client._sock = None

        with patch("gui.protocol_client.socket.socket", return_value=fake):
            response = client.send_once("TOOL_INFO", "python")

        self.assertTrue(response.ok)
        payload = json.loads(response.payload)
        self.assertEqual(payload["tool"]["descriptor"]["name"], "python")
        self.assertEqual(payload["sandbox"]["timeout_s"], 8)


if __name__ == "__main__":
    unittest.main()
