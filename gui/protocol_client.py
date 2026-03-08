from __future__ import annotations

import socket
import threading
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
    def __init__(self, host: str = "127.0.0.1", port: int = 0):
        import os
        self.host = host
        self.port = port if port else int(os.environ.get("AGENTIC_PORT", "6380"))
        self._auth_token: Optional[str] = None
        # Persistent socket for control-plane requests (protected by lock)
        self._sock: Optional[socket.socket] = None
        self._lock = threading.Lock()

    def _load_token(self) -> str:
        """Read auth token from workspace/.kernel_token (cached)."""
        if self._auth_token is not None:
            return self._auth_token
        import os
        token_path = os.path.join("workspace", ".kernel_token")
        try:
            with open(token_path, "r") as f:
                self._auth_token = f.read().strip()
        except FileNotFoundError:
            self._auth_token = ""
        return self._auth_token

    def _authenticate(self, sock: socket.socket) -> None:
        """Send AUTH command on a freshly connected socket."""
        token = self._load_token()
        if not token:
            return
        auth_payload = token.encode("utf-8")
        auth_header = f"AUTH 1 {len(auth_payload)}\n".encode("utf-8")
        sock.sendall(auth_header)
        sock.sendall(auth_payload)
        # Read and discard the AUTH response
        buf = bytearray()
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            buf.extend(chunk)
            frames = consume_framed_messages(buf)
            if frames:
                break

    def reload_token(self) -> None:
        """Force re-read of the auth token on next connection."""
        self._auth_token = None

    def reset_session(self) -> None:
        """Drop socket and auth cache after kernel lifecycle changes."""
        with self._lock:
            self._close_persistent()
            self._auth_token = None

    def _ensure_connection(self, read_timeout_s: float) -> socket.socket:
        """Return the persistent socket, reconnecting if needed."""
        if self._sock is not None:
            try:
                # Quick liveness check — peek for errors
                self._sock.settimeout(read_timeout_s)
                return self._sock
            except OSError:
                self._close_persistent()

        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(read_timeout_s)
        sock.connect((self.host, self.port))
        self._authenticate(sock)
        self._sock = sock
        return sock

    def _close_persistent(self) -> None:
        """Close the persistent socket (caller must hold _lock)."""
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None

    def close(self) -> None:
        """Public close — for shutdown cleanup."""
        with self._lock:
            self._close_persistent()

    def send_once(
        self,
        verb: str,
        payload: str = "",
        agent_id: str = "1",
        read_timeout_s: float = 5.0,
        inactivity_timeout_s: float = 0.5,
    ) -> ControlResponse:
        with self._lock:
            return self._send_once_locked(verb, payload, agent_id, read_timeout_s, inactivity_timeout_s)

    def _send_once_locked(
        self,
        verb: str,
        payload: str,
        agent_id: str,
        read_timeout_s: float,
        inactivity_timeout_s: float,
    ) -> ControlResponse:
        payload_bytes = payload.encode("utf-8")
        header = f"{verb} {agent_id} {len(payload_bytes)}\n".encode("utf-8")

        start = time.perf_counter()
        frame_buffer = bytearray()
        control: Optional[ControlResponse] = None

        try:
            sock = self._ensure_connection(read_timeout_s)
            sock.sendall(header)
            if payload_bytes:
                sock.sendall(payload_bytes)

            last_data_at = time.perf_counter()
            while True:
                try:
                    chunk = sock.recv(4096)
                    if not chunk:
                        # Connection closed by kernel — force reconnect next time
                        self._close_persistent()
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
        except OSError:
            # Connection error — teardown so next call reconnects
            self._close_persistent()
            raise

        if control is None:
            code = "TIMEOUT"
            payload = "No complete control frame received before timeout."
            if frame_buffer:
                code = "MALFORMED_OR_PARTIAL"
                payload = "Received non-complete or malformed frame data."
            return ControlResponse(
                ok=False,
                code=code,
                payload=payload,
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
        inactivity_timeout_s: float = 8.0,
    ) -> ControlResponse:
        payload_bytes = prompt.encode("utf-8")
        header = f"EXEC {agent_id} {len(payload_bytes)}\n".encode("utf-8")

        start = time.perf_counter()
        frame_buffer = bytearray()
        control: Optional[ControlResponse] = None
        saw_process_finished = False

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(connect_timeout_s)
            sock.connect((self.host, self.port))
            self._authenticate(sock)
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
                        if kind == "DATA" and code.lower() == "raw":
                            text = body.decode("utf-8", errors="replace")
                            if "[PROCESS_FINISHED" in text:
                                saw_process_finished = True
                        if kind in {"+OK", "-ERR"}:
                            control = ControlResponse(
                                ok=(kind == "+OK"),
                                code=code,
                                payload=body.decode("utf-8", errors="replace"),
                                duration_s=time.perf_counter() - start,
                            )
                except socket.timeout:
                    continue

        if control is None and saw_process_finished:
            return ControlResponse(
                ok=True,
                code="PROCESS_FINISHED",
                payload="Process finished marker received.",
                duration_s=time.perf_counter() - start,
            )

        if control is None:
            return ControlResponse(
                ok=False,
                code="EXEC_INCOMPLETE",
                payload="Stream closed before final control frame.",
                duration_s=time.perf_counter() - start,
            )

        return control
