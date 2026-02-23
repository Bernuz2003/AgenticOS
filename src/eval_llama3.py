import argparse
import json
import socket
import subprocess
import time
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Optional

HOST = "127.0.0.1"
PORT = 6379
FINISHED_MARKER = "[PROCESS_FINISHED pid="


@dataclass
class CommandResult:
    verb: str
    ok: bool
    code: str
    response: str
    duration_s: float


@dataclass
class ExecResult:
    name: str
    prompt: str
    ok: bool
    first_chunk_s: Optional[float]
    total_s: float
    bytes_received: int
    output_preview: str
    output_tail: str
    raw_output: str
    control_code: str


def _read_control_frame(data: bytes) -> tuple[bool, str, str]:
    line_end = data.find(b"\r\n")
    if line_end == -1:
        text = data.decode("utf-8", errors="replace")
        return text.startswith("+OK"), "MALFORMED", text

    header = data[:line_end].decode("utf-8", errors="replace")
    parts = header.split()
    if len(parts) < 3:
        return False, "MALFORMED", data.decode("utf-8", errors="replace")

    status = parts[0]
    code = parts[1]
    try:
        payload_len = int(parts[2])
    except ValueError:
        return False, "MALFORMED", data.decode("utf-8", errors="replace")

    start = line_end + 2
    end = min(start + payload_len, len(data))
    payload = data[start:end].decode("utf-8", errors="replace")
    return status == "+OK", code, payload


def _consume_framed_messages(buffer: bytearray) -> list[tuple[str, str, bytes]]:
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


def _extract_data_payloads(raw: bytes) -> tuple[str, str]:
    idx = 0
    out_parts: list[str] = []
    control_code = "MISSING"

    while idx < len(raw):
        line_end = raw.find(b"\r\n", idx)
        if line_end == -1:
            break

        header = raw[idx:line_end].decode("utf-8", errors="replace")
        parts = header.split()
        if len(parts) < 3:
            break

        if parts[0] in {"+OK", "-ERR"}:
            control_code = parts[1]
            try:
                payload_len = int(parts[2])
            except ValueError:
                break
            start = line_end + 2
            idx = start + payload_len
            continue

        if parts[0] == "DATA" and len(parts) >= 3 and parts[1].lower() == "raw":
            try:
                payload_len = int(parts[2])
            except ValueError:
                break
            start = line_end + 2
            end = start + payload_len
            if end > len(raw):
                break
            out_parts.append(raw[start:end].decode("utf-8", errors="replace"))
            idx = end
            continue

        break

    return "".join(out_parts), control_code


def git_metadata() -> dict:
    def run(cmd: list[str]) -> str:
        try:
            res = subprocess.run(cmd, check=True, capture_output=True, text=True)
            return res.stdout.strip()
        except Exception:
            return "unknown"

    commit = run(["git", "rev-parse", "--short", "HEAD"])
    branch = run(["git", "rev-parse", "--abbrev-ref", "HEAD"])
    dirty = run(["git", "status", "--porcelain"]) != ""
    return {"commit": commit, "branch": branch, "dirty": dirty}


class KernelClient:
    def __init__(self, host: str, port: int):
        self.host = host
        self.port = port

    def send_once(
        self,
        verb: str,
        payload: str,
        agent_id: str = "1",
        read_timeout_s: float = 2.0,
        inactivity_timeout_s: float = 0.8,
    ) -> CommandResult:
        payload_bytes = payload.encode("utf-8")
        header = f"{verb} {agent_id} {len(payload_bytes)}\n".encode("utf-8")
        start = time.perf_counter()

        data_chunks = []
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(read_timeout_s)
            sock.connect((self.host, self.port))
            sock.sendall(header)
            if payload_bytes:
                sock.sendall(payload_bytes)

            last_data_at = time.perf_counter()
            frame_buffer = bytearray()
            control_result: tuple[bool, str, str] | None = None
            while True:
                try:
                    chunk = sock.recv(4096)
                    if not chunk:
                        break
                    data_chunks.append(chunk)
                    frame_buffer.extend(chunk)
                    last_data_at = time.perf_counter()

                    for kind, code, payload in _consume_framed_messages(frame_buffer):
                        if kind in {"+OK", "-ERR"}:
                            control_result = (kind == "+OK", code, payload.decode("utf-8", errors="replace"))
                            break
                    if control_result is not None:
                        break
                except socket.timeout:
                    if time.perf_counter() - last_data_at >= inactivity_timeout_s:
                        break

        total_s = time.perf_counter() - start
        if control_result is not None:
            ok, code, payload = control_result
        else:
            raw = b"".join(data_chunks)
            ok, code, payload = _read_control_frame(raw)
        return CommandResult(verb=verb, ok=ok, code=code, response=payload, duration_s=total_s)

    def exec_stream(
        self,
        prompt: str,
        agent_id: str = "1",
        connect_timeout_s: float = 10.0,
        first_byte_timeout_s: float = 45.0,
        inactivity_timeout_s: float = 3.0,
        max_total_s: float = 90.0,
    ) -> ExecResult:
        payload_bytes = prompt.encode("utf-8")
        header = f"EXEC {agent_id} {len(payload_bytes)}\n".encode("utf-8")

        chunks = []
        start = time.perf_counter()
        first_chunk_s = None
        streamed_parts: list[str] = []
        control_code = "MISSING"
        frame_buffer = bytearray()
        saw_finished = False

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(connect_timeout_s)
            sock.connect((self.host, self.port))
            sock.sendall(header)
            sock.sendall(payload_bytes)

            sock.settimeout(0.7)
            last_data_at = time.perf_counter()

            while True:
                now = time.perf_counter()
                if now - start > max_total_s:
                    break
                if first_chunk_s is None and now - start > first_byte_timeout_s:
                    break
                if first_chunk_s is not None and now - last_data_at > inactivity_timeout_s:
                    break

                try:
                    chunk = sock.recv(4096)
                    if not chunk:
                        break
                    if first_chunk_s is None:
                        first_chunk_s = now - start
                    chunks.append(chunk)
                    frame_buffer.extend(chunk)
                    last_data_at = time.perf_counter()

                    frames = _consume_framed_messages(frame_buffer)
                    for kind, code, payload in frames:
                        if kind in {"+OK", "-ERR"}:
                            control_code = code
                            continue

                        if kind == "DATA" and code.lower() == "raw":
                            text = payload.decode("utf-8", errors="replace")
                            streamed_parts.append(text)
                            if FINISHED_MARKER in text:
                                saw_finished = True
                    if saw_finished:
                        break
                except socket.timeout:
                    continue

        total_s = time.perf_counter() - start
        raw_bytes = b"".join(chunks)
        raw_output = raw_bytes.decode("utf-8", errors="replace")
        bytes_received = sum(len(c) for c in chunks)
        streamed_text = "".join(streamed_parts)
        if not streamed_text:
            streamed_text, extracted_control = _extract_data_payloads(raw_bytes)
            if control_code == "MISSING":
                control_code = extracted_control

        return ExecResult(
            name="",
            prompt=prompt,
            ok=(control_code != "MISSING" and "-ERR" not in raw_output and bytes_received > 0),
            first_chunk_s=first_chunk_s,
            total_s=total_s,
            bytes_received=bytes_received,
            output_preview=streamed_text[:400],
            output_tail=streamed_text[-400:],
            raw_output=raw_output,
            control_code=control_code,
        )


def llama3_chat_prompt(user_text: str, system_text: str = "You are a precise and concise assistant.") -> str:
    return (
        "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n"
        f"{system_text}\n"
        "<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n"
        f"{user_text}\n"
        "<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n"
    )


def default_prompt_suite() -> list[tuple[str, str]]:
    return [
        ("brief_italian", "Spiegami in 3 frasi cosa fa un kernel event-driven."),
        ("structured_reasoning", "Dammi 5 punti chiari su come progettare un protocollo TCP robusto."),
        (
            "syscall_calc",
            "Calcola 37*19 usando questo formato tool: [[CALC: 37*19]] e poi spiegami il risultato in 1 frase.",
        ),
        (
            "syscall_ls",
            "Usa [[LS]] e riassumi i file trovati in massimo 2 righe.",
        ),
    ]


def evaluate(args: argparse.Namespace) -> dict:
    client = KernelClient(args.host, args.port)
    git = git_metadata()

    ping = client.send_once(
        "PING",
        "",
        read_timeout_s=args.ping_timeout,
        inactivity_timeout_s=args.ping_inactivity,
    )
    load = client.send_once(
        "LOAD",
        args.model_path,
        read_timeout_s=args.load_timeout,
        inactivity_timeout_s=args.load_inactivity,
    )

    runs: list[ExecResult] = []
    for run_idx in range(args.run_count):
        for name, user_text in default_prompt_suite():
            prompt = llama3_chat_prompt(user_text)
            result = client.exec_stream(
                prompt=prompt,
                first_byte_timeout_s=args.first_byte_timeout,
                inactivity_timeout_s=args.inactivity_timeout,
                max_total_s=args.max_total,
            )
            result.name = f"{name}#run{run_idx+1}"
            runs.append(result)

    benchmark_id = f"{time.strftime('%Y%m%d-%H%M%S')}-{git['commit']}"

    summary = {
        "benchmark_id": benchmark_id,
        "host": args.host,
        "port": args.port,
        "model_path": args.model_path,
        "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
        "git": git,
        "ping": asdict(ping),
        "load": asdict(load),
        "runs": [asdict(r) for r in runs],
        "thresholds": {
            "load_ok": True,
            "first_chunk_max_s": args.threshold_first_chunk,
            "min_bytes_each_run": args.threshold_min_bytes,
            "max_total_s": args.threshold_total,
            "max_failed_runs": args.threshold_failed_runs,
        },
    }

    failed_runs = 0
    for run in runs:
        if (
            not run.ok
            or run.first_chunk_s is None
            or run.first_chunk_s > args.threshold_first_chunk
            or run.bytes_received < args.threshold_min_bytes
            or run.total_s > args.threshold_total
        ):
            failed_runs += 1

    summary["evaluation"] = {
        "pass": load.ok and failed_runs <= args.threshold_failed_runs,
        "failed_runs": failed_runs,
        "total_runs": len(runs),
    }

    return summary


def main() -> None:
    parser = argparse.ArgumentParser(description="Evaluate AgenticOS kernel behavior with Llama 3 8B")
    parser.add_argument("--host", default=HOST)
    parser.add_argument("--port", type=int, default=PORT)
    parser.add_argument("--model-path", default="models/Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf")
    parser.add_argument("--inactivity-timeout", type=float, default=6.0)
    parser.add_argument("--first-byte-timeout", type=float, default=60.0)
    parser.add_argument("--max-total", type=float, default=180.0)
    parser.add_argument("--ping-timeout", type=float, default=3.0)
    parser.add_argument("--ping-inactivity", type=float, default=1.0)
    parser.add_argument("--load-timeout", type=float, default=90.0)
    parser.add_argument("--load-inactivity", type=float, default=3.0)

    parser.add_argument("--threshold-first-chunk", type=float, default=55.0)
    parser.add_argument("--threshold-min-bytes", type=int, default=25)
    parser.add_argument("--threshold-total", type=float, default=180.0)
    parser.add_argument("--threshold-failed-runs", type=int, default=2)

    parser.add_argument("--report-path", default="reports/llama3_eval_report.json")
    parser.add_argument("--run-count", type=int, default=1)
    args = parser.parse_args()

    summary = evaluate(args)

    report_path = Path(args.report_path)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")

    print(f"Report written to: {report_path}")
    print(json.dumps(summary["evaluation"], indent=2))


if __name__ == "__main__":
    main()
