#!/usr/bin/env python3
import argparse
import json
import socket
import sys
from pathlib import Path
from typing import Optional


HELLO_REQ = {
    "supported_versions": ["v1"],
    "required_capabilities": [],
}


def encode_command(opcode: str, agent_id: str, payload: bytes) -> bytes:
    header = f"{opcode} {agent_id} {len(payload)}\n".encode("utf-8")
    return header + payload


def recv_frame(sock: socket.socket, buffer: bytearray) -> Optional[tuple[str, str, bytes]]:
    while True:
        sep = buffer.find(b"\r\n")
        if sep != -1:
            header = buffer[:sep].decode("utf-8", errors="replace")
            parts = header.split()
            if len(parts) < 3:
                raise RuntimeError(f"Malformed response header: {header!r}")
            kind, code, payload_len_raw = parts[0], parts[1], parts[2]
            payload_len = int(payload_len_raw)
            total = sep + 2 + payload_len
            if len(buffer) >= total:
                payload = bytes(buffer[sep + 2 : total])
                del buffer[:total]
                return kind, code, payload

        try:
            chunk = sock.recv(4096)
        except socket.timeout:
            return None

        if not chunk:
            raise EOFError("Connection closed by kernel")

        buffer.extend(chunk)


def maybe_json(payload: bytes):
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError:
        return None, None
    try:
        parsed = json.loads(text)
        return text, parsed
    except json.JSONDecodeError:
        return text, None


def print_frame(kind: str, code: str, payload: bytes, pretty_json: bool):
    print(f"\n=== FRAME {kind} {code} len={len(payload)} ===")
    text, parsed = maybe_json(payload)
    if parsed is not None:
        if pretty_json:
            print(json.dumps(parsed, indent=2, ensure_ascii=False))
        else:
            print(text)
    elif text is not None:
        print(text)
    else:
        print(payload)


def frame_record(kind: str, code: str, payload: bytes):
    text, parsed = maybe_json(payload)
    return {
        "kind": kind,
        "code": code,
        "payload_len": len(payload),
        "payload_text": text,
        "payload_json": parsed,
    }


def send_and_expect_ok(
    sock: socket.socket,
    buffer: bytearray,
    opcode: str,
    agent_id: str,
    payload_bytes: bytes,
    pretty_json: bool,
    dump_file,
):
    sock.sendall(encode_command(opcode, agent_id, payload_bytes))
    frame = recv_frame(sock, buffer)
    if frame is None:
        raise RuntimeError(f"Timed out waiting for response to {opcode}")
    kind, code, payload = frame
    print_frame(kind, code, payload, pretty_json)
    if dump_file is not None:
        dump_file.write(json.dumps(frame_record(kind, code, payload), ensure_ascii=False) + "\n")
        dump_file.flush()
    if kind != "+OK":
        raise RuntimeError(f"{opcode} failed: {kind} {code}")
    return kind, code, payload


def parse_args():
    parser = argparse.ArgumentParser(description="AgenticOS direct TCP inference client")
    parser.add_argument("--workspace-root", required=True)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=6380)
    parser.add_argument("--model", default="")
    parser.add_argument("--load", action="store_true")
    parser.add_argument("--agent-id", default="1")
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--idle-frames", type=int, default=3)
    parser.add_argument("--raw-json", action="store_true")
    parser.add_argument("--dump-jsonl", default="")
    return parser.parse_args()


def main():
    args = parse_args()
    token_path = Path(args.workspace_root) / "workspace/.kernel_token"
    token = token_path.read_text(encoding="utf-8").strip()
    if not token:
        raise RuntimeError(f"Kernel token is empty: {token_path}")

    dump_file = None
    if args.dump_jsonl:
        dump_path = Path(args.dump_jsonl)
        dump_path.parent.mkdir(parents=True, exist_ok=True)
        dump_file = dump_path.open("w", encoding="utf-8")

    print(f"[info] Connecting to {args.host}:{args.port}")
    buffer = bytearray()
    try:
        with socket.create_connection((args.host, args.port), timeout=5.0) as sock:
            sock.settimeout(args.timeout)

            print("[info] AUTH")
            send_and_expect_ok(
                sock,
                buffer,
                "AUTH",
                args.agent_id,
                token.encode("utf-8"),
                not args.raw_json,
                dump_file,
            )

            print("[info] HELLO")
            send_and_expect_ok(
                sock,
                buffer,
                "HELLO",
                args.agent_id,
                json.dumps(HELLO_REQ).encode("utf-8"),
                not args.raw_json,
                dump_file,
            )

            if args.model:
                print(f"[info] SELECT_MODEL {args.model}")
                send_and_expect_ok(
                    sock,
                    buffer,
                    "SELECT_MODEL",
                    args.agent_id,
                    args.model.encode("utf-8"),
                    not args.raw_json,
                    dump_file,
                )

            if args.load:
                selector = args.model if args.model else ""
                print(f"[info] LOAD {selector!r}")
                send_and_expect_ok(
                    sock,
                    buffer,
                    "LOAD",
                    args.agent_id,
                    selector.encode("utf-8"),
                    not args.raw_json,
                    dump_file,
                )

            print("[info] EXEC")
            send_and_expect_ok(
                sock,
                buffer,
                "EXEC",
                args.agent_id,
                args.prompt.encode("utf-8"),
                not args.raw_json,
                dump_file,
            )

            print("[info] Reading stream frames... (Ctrl+C to stop)")
            idle = 0
            while True:
                frame = recv_frame(sock, buffer)
                if frame is None:
                    idle += 1
                    if idle >= args.idle_frames:
                        print(
                            f"\n[info] No new frames for {args.idle_frames} consecutive timeouts. Stopping."
                        )
                        break
                    continue

                idle = 0
                kind, code, payload = frame
                print_frame(kind, code, payload, not args.raw_json)
                if dump_file is not None:
                    dump_file.write(
                        json.dumps(frame_record(kind, code, payload), ensure_ascii=False) + "\n"
                    )
                    dump_file.flush()
    finally:
        if dump_file is not None:
            dump_file.close()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n[info] Interrupted by user", file=sys.stderr)
        sys.exit(130)
    except Exception as exc:
        print(f"[error] {exc}", file=sys.stderr)
        sys.exit(1)
