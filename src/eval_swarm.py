#!/usr/bin/env python3
"""Benchmark comparativo: single-model vs capability-aware swarm routing.

Connects to a running AgenticOS kernel (default 127.0.0.1:6379) and runs a
suite of prompts under two scenarios:

  (A) **single_model** — one model is selected via SELECT_MODEL, EXEC_AUTO_SWITCH
      is implicitly off.  All tasks go to the same model.

  (B) **swarm_routing** — EXEC_AUTO_SWITCH is simulated by using the
      `capability=<hint>;` prefix on each EXEC payload, letting the kernel
      scheduler pick the best model per-workload.

For each scenario the script measures: latency per task, first-token latency,
throughput (bytes/s), and produces p50/p95 aggregates plus a task-completion
rate.

Usage (kernel must be running):

    python3 src/eval_swarm.py [--host 127.0.0.1] [--port 6379]

Output: reports/swarm_benchmark.json
"""

import argparse
import json
import os
import socket
import statistics
import subprocess
import time
from dataclasses import dataclass, asdict, field
from pathlib import Path
from typing import Optional

HOST = "127.0.0.1"
PORT = 6379
FINISHED_MARKER = "[PROCESS_FINISHED pid="


# ── Protocol helpers (adapted from eval_llama3.py) ──────────────────────


def _consume_framed_messages(buffer: bytearray) -> list[tuple[str, str, bytes]]:
    """Parse the kernel wire format: ``+OK CODE LEN\\r\\n<payload>`` /
    ``DATA raw LEN\\r\\n<bytes>``."""
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
        if kind not in {"+OK", "-ERR", "DATA"}:
            break
        try:
            payload_len = int(parts[2])
        except ValueError:
            break
        total_needed = line_end + 2 + payload_len
        if len(buffer) < total_needed:
            break
        payload = bytes(buffer[line_end + 2:total_needed])
        del buffer[:total_needed]
        frames.append((kind, parts[1], payload))
    return frames


@dataclass
class CmdResult:
    ok: bool
    code: str
    payload: str
    duration_s: float


def send_cmd(host: str, port: int, verb: str, payload: str = "",
             agent_id: str = "bench", timeout_s: float = 120.0,
             inactivity_s: float = 3.0) -> CmdResult:
    """Send a non-streaming command and return the first control frame."""
    payload_bytes = payload.encode("utf-8")
    header = f"{verb} {agent_id} {len(payload_bytes)}\n".encode("utf-8")
    start = time.perf_counter()

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(timeout_s)
        sock.connect((host, port))
        sock.sendall(header + payload_bytes)

        buf = bytearray()
        last_data = time.perf_counter()
        while True:
            try:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                buf.extend(chunk)
                last_data = time.perf_counter()
                for kind, code, data in _consume_framed_messages(buf):
                    if kind in {"+OK", "-ERR"}:
                        return CmdResult(
                            ok=(kind == "+OK"), code=code,
                            payload=data.decode("utf-8", errors="replace"),
                            duration_s=time.perf_counter() - start,
                        )
            except socket.timeout:
                if time.perf_counter() - last_data > inactivity_s:
                    break

    return CmdResult(ok=False, code="TIMEOUT", payload="", duration_s=time.perf_counter() - start)


@dataclass
class ExecMetrics:
    """Metrics for one EXEC invocation."""
    task_name: str
    workload: str
    ok: bool
    first_token_s: Optional[float]
    total_s: float
    bytes_received: int
    tokens_approx: int  # rough: bytes / 4
    output_preview: str = ""
    error: str = ""


def exec_stream(host: str, port: int, prompt: str, task_name: str,
                workload: str, agent_id: str = "bench",
                first_byte_timeout_s: float = 60.0,
                inactivity_s: float = 8.0,
                max_total_s: float = 240.0) -> ExecMetrics:
    """Send EXEC, stream tokens, return latency + throughput metrics."""
    payload_bytes = prompt.encode("utf-8")
    header = f"EXEC {agent_id} {len(payload_bytes)}\n".encode("utf-8")

    start = time.perf_counter()
    first_token_s: Optional[float] = None
    total_bytes = 0
    streamed: list[str] = []
    saw_finished = False
    buf = bytearray()
    error = ""

    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(first_byte_timeout_s)
            sock.connect((host, port))
            sock.sendall(header + payload_bytes)
            sock.settimeout(0.7)
            last_data = time.perf_counter()

            while True:
                now = time.perf_counter()
                if now - start > max_total_s:
                    error = "max_total exceeded"
                    break
                if first_token_s is None and now - start > first_byte_timeout_s:
                    error = "first_byte timeout"
                    break
                if first_token_s is not None and now - last_data > inactivity_s:
                    break  # normal end (inactivity after output started)
                try:
                    chunk = sock.recv(4096)
                    if not chunk:
                        break
                    total_bytes += len(chunk)
                    buf.extend(chunk)
                    last_data = time.perf_counter()
                    if first_token_s is None:
                        first_token_s = now - start

                    for kind, code, payload in _consume_framed_messages(buf):
                        if kind == "-ERR":
                            error = f"{code}: {payload.decode('utf-8', errors='replace')}"
                        if kind == "DATA" and code.lower() == "raw":
                            text = payload.decode("utf-8", errors="replace")
                            streamed.append(text)
                            if FINISHED_MARKER in text:
                                saw_finished = True
                    if saw_finished:
                        break
                except socket.timeout:
                    continue
    except Exception as e:
        error = str(e)

    total_s = time.perf_counter() - start
    output_text = "".join(streamed)
    ok = saw_finished and not error

    return ExecMetrics(
        task_name=task_name,
        workload=workload,
        ok=ok,
        first_token_s=first_token_s,
        total_s=total_s,
        bytes_received=total_bytes,
        tokens_approx=max(total_bytes // 4, 0),
        output_preview=output_text[:300],
        error=error,
    )


# ── Prompt suite ─────────────────────────────────────────────────────────

TASK_SUITE: list[dict] = [
    {
        "name": "brief_kernel",
        "prompt": "Spiegami in 3 frasi cosa fa un kernel event-driven.",
        "workload": "fast",
    },
    {
        "name": "code_fibonacci",
        "prompt": "Scrivi una funzione Rust che calcola il n-esimo numero di Fibonacci con memoization.",
        "workload": "code",
    },
    {
        "name": "reasoning_compare",
        "prompt": "Ragiona passo-passo: è più efficiente usare un DAG o un albero per orchestrare task paralleli? Giustifica la risposta.",
        "workload": "reasoning",
    },
    {
        "name": "general_summary",
        "prompt": "Riassumi in 5 punti i vantaggi di un sistema operativo agentico rispetto a un agente LLM monolitico.",
        "workload": "general",
    },
    {
        "name": "code_python_sort",
        "prompt": "Scrivi in Python una merge-sort che accetta una lista di dict e ordina per una chiave arbitraria.",
        "workload": "code",
    },
    {
        "name": "fast_translate",
        "prompt": "Traduci in inglese: 'Il kernel gestisce la memoria e lo scheduling dei processi agentici.'",
        "workload": "fast",
    },
]


# ── Benchmark scenarios ──────────────────────────────────────────────────


def git_metadata() -> dict:
    def run(cmd: list[str]) -> str:
        try:
            res = subprocess.run(cmd, check=True, capture_output=True, text=True)
            return res.stdout.strip()
        except Exception:
            return "unknown"

    return {
        "commit": run(["git", "rev-parse", "--short", "HEAD"]),
        "branch": run(["git", "rev-parse", "--abbrev-ref", "HEAD"]),
        "dirty": run(["git", "status", "--porcelain"]) != "",
    }


def run_scenario(
    host: str, port: int, scenario_name: str,
    model_id: Optional[str], use_capability_hint: bool,
    repeat: int, max_tokens: int = 128,
) -> dict:
    """Run the full task suite under one scenario configuration."""
    results: list[dict] = []

    # Limit output length so tasks complete within the timeout on CPU.
    gen_res = send_cmd(host, port, "SET_GEN", f"max_tokens={max_tokens}")
    if not gen_res.ok:
        print(f"  WARN: SET_GEN max_tokens={max_tokens} failed: {gen_res.payload}")

    # If a specific model is forced, select it first.
    if model_id:
        sel = send_cmd(host, port, "SELECT_MODEL", model_id)
        if not sel.ok:
            return {"scenario": scenario_name, "error": f"SELECT_MODEL failed: {sel.payload}",
                    "results": []}
        # Force-load it.
        load = send_cmd(host, port, "LOAD", model_id, timeout_s=120)
        if not load.ok:
            return {"scenario": scenario_name, "error": f"LOAD failed: {load.payload}",
                    "results": []}

    for run_idx in range(repeat):
        for task in TASK_SUITE:
            prompt_text = task["prompt"]
            workload = task["workload"]

            if use_capability_hint:
                prompt = f"capability={workload}; {prompt_text}"
            else:
                prompt = prompt_text

            metrics = exec_stream(
                host, port, prompt,
                task_name=f"{task['name']}#run{run_idx + 1}",
                workload=workload,
            )
            results.append(asdict(metrics))

    # ── Aggregates ───────────────────────────────────────────────────
    ok_runs = [r for r in results if r["ok"]]
    total_runs = len(results)
    completed = len(ok_runs)

    latencies = [r["total_s"] for r in ok_runs]
    first_tokens = [r["first_token_s"] for r in ok_runs if r["first_token_s"] is not None]
    throughputs = [r["bytes_received"] / r["total_s"] for r in ok_runs if r["total_s"] > 0]

    def percentile(data: list[float], pct: float) -> Optional[float]:
        if not data:
            return None
        s = sorted(data)
        idx = int(len(s) * pct / 100)
        idx = min(idx, len(s) - 1)
        return round(s[idx], 4)

    aggregates = {
        "total_tasks": total_runs,
        "completed": completed,
        "failed": total_runs - completed,
        "completion_rate": round(completed / total_runs, 4) if total_runs > 0 else 0,
        "latency_p50_s": percentile(latencies, 50),
        "latency_p95_s": percentile(latencies, 95),
        "latency_mean_s": round(statistics.mean(latencies), 4) if latencies else None,
        "first_token_p50_s": percentile(first_tokens, 50),
        "first_token_p95_s": percentile(first_tokens, 95),
        "throughput_bytes_per_s_p50": percentile(throughputs, 50),
        "throughput_bytes_per_s_p95": percentile(throughputs, 95),
        "total_bytes": sum(r["bytes_received"] for r in ok_runs),
        "total_time_s": round(sum(r["total_s"] for r in results), 4),
    }

    return {
        "scenario": scenario_name,
        "model_id": model_id,
        "use_capability_hint": use_capability_hint,
        "runs": results,
        "aggregates": aggregates,
    }


def regression_analysis(single: dict, swarm: dict) -> dict:
    """Compute deltas between single-model and swarm scenarios."""
    s_agg = single.get("aggregates", {})
    w_agg = swarm.get("aggregates", {})

    def delta(key: str, lower_is_better: bool = True) -> Optional[dict]:
        s_val = s_agg.get(key)
        w_val = w_agg.get(key)
        if s_val is None or w_val is None:
            return None
        diff = round(w_val - s_val, 4)
        pct = round(diff / s_val * 100, 2) if s_val != 0 else None
        if lower_is_better:
            verdict = "improved" if diff < 0 else ("same" if diff == 0 else "regressed")
        else:
            verdict = "improved" if diff > 0 else ("same" if diff == 0 else "regressed")
        return {"single": s_val, "swarm": w_val, "delta": diff, "pct": pct, "verdict": verdict}

    return {
        "latency_p50": delta("latency_p50_s"),
        "latency_p95": delta("latency_p95_s"),
        "latency_mean": delta("latency_mean_s"),
        "first_token_p50": delta("first_token_p50_s"),
        "first_token_p95": delta("first_token_p95_s"),
        "throughput_p50": delta("throughput_bytes_per_s_p50", lower_is_better=False),
        "throughput_p95": delta("throughput_bytes_per_s_p95", lower_is_better=False),
        "completion_rate": delta("completion_rate", lower_is_better=False),
    }


# ── Main ─────────────────────────────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Benchmark comparativo: single-model vs capability-aware swarm routing",
    )
    parser.add_argument("--host", default=HOST)
    parser.add_argument("--port", type=int, default=PORT)
    parser.add_argument("--single-model", default=None,
                        help="Model ID for single-model scenario (default: auto-detect first model)")
    parser.add_argument("--repeat", type=int, default=1,
                        help="Repeat task suite N times per scenario")
    parser.add_argument("--report-path", default="reports/swarm_benchmark.json")
    args = parser.parse_args()

    print("=" * 60)
    print("AgenticOS — Swarm Benchmark")
    print("=" * 60)

    # ── Connectivity check ───────────────────────────────────────────
    ping = send_cmd(args.host, args.port, "PING")
    if not ping.ok:
        print(f"ERROR: Cannot reach kernel at {args.host}:{args.port}: {ping.payload}")
        raise SystemExit(1)
    print(f"✓ Kernel reachable ({ping.duration_s:.3f}s)")

    # ── Discover models ──────────────────────────────────────────────
    models = send_cmd(args.host, args.port, "LIST_MODELS")
    if not models.ok:
        print(f"ERROR: LIST_MODELS failed: {models.payload}")
        raise SystemExit(1)
    print(f"✓ Models discovered:\n{models.payload}")

    # Auto-detect first model for single-model scenario.
    model_lines = [l.strip() for l in models.payload.strip().splitlines() if l.strip()]
    available_ids = []
    for line in model_lines:
        # Lines like: "- id=llama3.1-8b/... family=Llama path=..."
        if "id=" in line:
            after = line.split("id=", 1)[1]
            model_id = after.split()[0] if after else ""
            if model_id:
                available_ids.append(model_id)

    if len(available_ids) < 1:
        print("ERROR: Need at least 1 model in models/ directory")
        raise SystemExit(1)

    single_model_id = args.single_model or available_ids[0]
    print(f"\n→ Single-model scenario: {single_model_id}")
    print(f"→ Tasks per scenario: {len(TASK_SUITE)} × {args.repeat} repeats\n")

    # ── Scenario A: Single model ─────────────────────────────────────
    print("-" * 50)
    print("SCENARIO A: Single model (no routing)")
    print("-" * 50)
    scenario_single = run_scenario(
        args.host, args.port,
        scenario_name="single_model",
        model_id=single_model_id,
        use_capability_hint=False,
        repeat=args.repeat,
    )
    a_agg = scenario_single.get("aggregates", {})
    print(f"  completed: {a_agg.get('completed', '?')}/{a_agg.get('total_tasks', '?')}")
    print(f"  latency p50: {a_agg.get('latency_p50_s', 'N/A')}s  p95: {a_agg.get('latency_p95_s', 'N/A')}s")
    print(f"  throughput p50: {a_agg.get('throughput_bytes_per_s_p50', 'N/A')} B/s")
    print()

    # ── Scenario B: Swarm routing ────────────────────────────────────
    # DISABLED: Qwen 14B (~9 GB) causes OOM on 31 GB systems when
    # combined with OS + IDE overhead.  The swarm scenario triggers
    # model switches to Qwen for code/reasoning workloads, saturating
    # RAM and crashing the host.  Re-enable when running on ≥64 GB RAM
    # or with a GPU offload path.
    #
    # print("-" * 50)
    # print("SCENARIO B: Swarm (capability-aware routing)")
    # print("-" * 50)
    # scenario_swarm = run_scenario(
    #     args.host, args.port,
    #     scenario_name="swarm_routing",
    #     model_id=None,
    #     use_capability_hint=True,
    #     repeat=args.repeat,
    # )
    scenario_swarm = {
        "scenario": "swarm_routing",
        "skipped": True,
        "reason": "Qwen 14B OOM on 31 GB system — needs ≥64 GB RAM or GPU",
        "aggregates": {},
    }
    print("  [SKIPPED] Swarm scenario disabled — Qwen 14B exceeds available RAM")
    print()

    # ── Regression analysis (partial — swarm skipped) ────────────────
    regression = regression_analysis(scenario_single, scenario_swarm)

    # ── Build report ─────────────────────────────────────────────────
    git = git_metadata()
    report = {
        "benchmark_id": f"swarm-{time.strftime('%Y%m%d-%H%M%S')}-{git['commit']}",
        "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
        "git": git,
        "kernel": {
            "host": args.host,
            "port": args.port,
            "models_available": available_ids,
        },
        "config": {
            "single_model_id": single_model_id,
            "task_count": len(TASK_SUITE),
            "repeat": args.repeat,
            "swarm_skipped": True,
            "swarm_skip_reason": "Qwen 14B OOM on 31 GB — drop-before-load not sufficient for 14B model",
        },
        "scenarios": {
            "single_model": scenario_single,
            "swarm_routing": scenario_swarm,
        },
        "regression": regression,
        "verdict": {
            "single_model_functional": a_agg.get("completion_rate", 0) > 0,
            "swarm_skipped": True,
            "latency_improved": None,
            "throughput_improved": None,
            "completion_maintained": None,
        },
    }

    # ── Save ─────────────────────────────────────────────────────────
    report_path = Path(args.report_path)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")

    # ── Summary ──────────────────────────────────────────────────────
    print("=" * 60)
    print("SINGLE-MODEL PERFORMANCE SUMMARY")
    print("=" * 60)
    for key, val in a_agg.items():
        print(f"  {key:35s}: {val}")
    print()
    if regression and any(v for v in regression.values()):
        print("REGRESSION ANALYSIS (vs swarm):")
        for metric, data in regression.items():
            if data:
                arrow = "↓" if data["verdict"] == "improved" else ("↑" if data["verdict"] == "regressed" else "=")
                pct_str = f" ({data['pct']:+.1f}%)" if data["pct"] is not None else ""
                print(f"  {metric:25s}: single={data['single']:<10}  swarm={data['swarm']:<10}  {arrow}{pct_str}  [{data['verdict']}]")
    else:
        print("  Regression analysis: N/A (swarm scenario skipped)")
    print()
    print(f"Report written to: {report_path}")
    print(f"Verdict: single_model_functional={report['verdict']['single_model_functional']}  "
          f"swarm_skipped={report['verdict']['swarm_skipped']}")


if __name__ == "__main__":
    main()
