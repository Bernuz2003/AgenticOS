import argparse
import json
import socket
import time
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Optional

HOST = "127.0.0.1"
PORT = 6379


@dataclass
class CommandResult:
    verb: str
    ok: bool
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
            while True:
                try:
                    chunk = sock.recv(4096)
                    if not chunk:
                        break
                    data_chunks.append(chunk)
                    last_data_at = time.perf_counter()
                except socket.timeout:
                    if time.perf_counter() - last_data_at >= inactivity_timeout_s:
                        break

        total_s = time.perf_counter() - start
        response = b"".join(data_chunks).decode("utf-8", errors="replace")
        ok = response.startswith("+OK")
        return CommandResult(verb=verb, ok=ok, response=response, duration_s=total_s)

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
                    last_data_at = time.perf_counter()
                except socket.timeout:
                    continue

        total_s = time.perf_counter() - start
        raw_output = b"".join(chunks).decode("utf-8", errors="replace")
        bytes_received = sum(len(c) for c in chunks)

        return ExecResult(
            name="",
            prompt=prompt,
            ok=("-ERR" not in raw_output and bytes_received > 0),
            first_chunk_s=first_chunk_s,
            total_s=total_s,
            bytes_received=bytes_received,
            output_preview=raw_output[:400],
            output_tail=raw_output[-400:],
            raw_output=raw_output,
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

    ping = client.send_once("PING", "")
    load = client.send_once("LOAD", args.model_path, read_timeout_s=5.0, inactivity_timeout_s=1.2)

    runs: list[ExecResult] = []
    for name, user_text in default_prompt_suite():
        prompt = llama3_chat_prompt(user_text)
        result = client.exec_stream(
            prompt=prompt,
            first_byte_timeout_s=args.first_byte_timeout,
            inactivity_timeout_s=args.inactivity_timeout,
            max_total_s=args.max_total,
        )
        result.name = name
        runs.append(result)

    summary = {
        "host": args.host,
        "port": args.port,
        "model_path": args.model_path,
        "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
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
    parser.add_argument("--model-path", default="models/Meta-Llama-3-8B-Instruct.Q4_K_M.gguf")
    parser.add_argument("--inactivity-timeout", type=float, default=3.0)
    parser.add_argument("--first-byte-timeout", type=float, default=45.0)
    parser.add_argument("--max-total", type=float, default=90.0)

    parser.add_argument("--threshold-first-chunk", type=float, default=30.0)
    parser.add_argument("--threshold-min-bytes", type=int, default=30)
    parser.add_argument("--threshold-total", type=float, default=90.0)
    parser.add_argument("--threshold-failed-runs", type=int, default=1)

    parser.add_argument("--report-path", default="reports/llama3_eval_report.json")
    args = parser.parse_args()

    summary = evaluate(args)

    report_path = Path(args.report_path)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")

    print(f"Report written to: {report_path}")
    print(json.dumps(summary["evaluation"], indent=2))


if __name__ == "__main__":
    main()
