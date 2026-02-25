from __future__ import annotations

import socket
import time
from dataclasses import dataclass
from typing import Callable, Optional

FrameCallback = Callable[[str, str, bytes], None]


@dataclass
class ControlResponse:
    ok: bool
    code: str
    payload: str
    duration_s: float


def consume_framed_messages(buffer: bytearray) -> list[tuple[str, str, bytes]]:
    frames: list[tuple[str, str, bytes]] = []

    while True:
        line_end = buffer.find(b"\r\n")
        if line_end == -1:
            break

        header = buffer[:line_end].decode("utf-8", errors="replace")
        parts = header.split()
        if len(parts) < 3:
            break

        kind = parts[0]
        if kind in {"+OK", "-ERR"}:
            code = parts[1]
            len_text = parts[2]
        elif kind == "DATA" and len(parts) >= 3:
            code = parts[1]
            len_text = parts[2]
        else:
            break

        try:
            payload_len = int(len_text)
        except ValueError:
            break

        total_needed = line_end + 2 + payload_len
        if len(buffer) < total_needed:
            break

        payload = bytes(buffer[line_end + 2 : total_needed])
        del buffer[:total_needed]
        frames.append((kind, code, payload))

    return frames


class ProtocolClient:
    def __init__(self, host: str = "127.0.0.1", port: int = 6379):
        self.host = host
        self.port = port

    def send_once(
        self,
        verb: str,
        payload: str = "",
        agent_id: str = "1",
        read_timeout_s: float = 5.0,
        inactivity_timeout_s: float = 0.5,
    ) -> ControlResponse:
        payload_bytes = payload.encode("utf-8")
        header = f"{verb} {agent_id} {len(payload_bytes)}\n".encode("utf-8")

        start = time.perf_counter()
        frame_buffer = bytearray()
        control: Optional[ControlResponse] = None

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(read_timeout_s)
            sock.connect((self.host, self.port))
            sock.sendall(header)
            if payload_bytes:
                sock.sendall(payload_bytes)

            last_data_at = time.perf_counter()
            while True:
                try:
                    chunk = sock.recv(4096)
                    if not chunk:
                        break
                    frame_buffer.extend(chunk)
                    last_data_at = time.perf_counter()

                    for kind, code, body in consume_framed_messages(frame_buffer):
                        if kind in {"+OK", "-ERR"}:
                            control = ControlResponse(
                                ok=(kind == "+OK"),
                                code=code,
                                payload=body.decode("utf-8", errors="replace"),
                                duration_s=time.perf_counter() - start,
                            )
                            break
                    if control is not None:
                        break
                except socket.timeout:
                    if time.perf_counter() - last_data_at >= inactivity_timeout_s:
                        break

        if control is None:
            return ControlResponse(
                ok=False,
                code="TIMEOUT_OR_MALFORMED",
                payload="No complete control frame received.",
                duration_s=time.perf_counter() - start,
            )

        return control

    def exec_stream(
        self,
        prompt: str,
        agent_id: str,
        on_frame: FrameCallback,
        connect_timeout_s: float = 10.0,
        max_total_s: float = 120.0,
        inactivity_timeout_s: float = 3.0,
    ) -> ControlResponse:
        payload_bytes = prompt.encode("utf-8")
        header = f"EXEC {agent_id} {len(payload_bytes)}\n".encode("utf-8")

        start = time.perf_counter()
        frame_buffer = bytearray()
        control: Optional[ControlResponse] = None

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(connect_timeout_s)
            sock.connect((self.host, self.port))
            sock.sendall(header)
            if payload_bytes:
                sock.sendall(payload_bytes)

            sock.settimeout(0.7)
            last_data_at = time.perf_counter()

            while True:
                now = time.perf_counter()
                if now - start > max_total_s:
                    break
                if now - last_data_at > inactivity_timeout_s:
                    break

                try:
                    chunk = sock.recv(4096)
                    if not chunk:
                        break

                    frame_buffer.extend(chunk)
                    last_data_at = time.perf_counter()

                    for kind, code, body in consume_framed_messages(frame_buffer):
                        on_frame(kind, code, body)
                        if kind in {"+OK", "-ERR"}:
                            control = ControlResponse(
                                ok=(kind == "+OK"),
                                code=code,
                                payload=body.decode("utf-8", errors="replace"),
                                duration_s=time.perf_counter() - start,
                            )
                except socket.timeout:
                    continue

        if control is None:
            return ControlResponse(
                ok=False,
                code="EXEC_INCOMPLETE",
                payload="Stream closed before final control frame.",
                duration_s=time.perf_counter() - start,
            )

        return control
