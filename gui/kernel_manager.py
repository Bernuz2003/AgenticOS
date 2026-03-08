from __future__ import annotations

import os
import queue
import subprocess
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


@dataclass
class KernelEvent:
    source: str
    line: str


class KernelProcessManager:
    def __init__(self, workspace_root: Path):
        self.workspace_root = workspace_root
        self.process: Optional[subprocess.Popen[str]] = None
        self.events: queue.Queue[KernelEvent] = queue.Queue()
        self._stdout_thread: Optional[threading.Thread] = None
        self._stderr_thread: Optional[threading.Thread] = None

    def start(self) -> tuple[bool, str]:
        if self.process is not None and self.process.poll() is None:
            return True, "Kernel già attivo."

        env = os.environ.copy()
        cmd, reason = self._resolve_start_command()

        try:
            self.process = subprocess.Popen(
                cmd,
                cwd=self.workspace_root,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
                env=env,
            )
        except Exception as exc:
            self.process = None
            return False, f"Avvio kernel fallito: {exc}"

        self._stdout_thread = threading.Thread(
            target=self._reader_loop,
            args=("stdout", self.process.stdout),
            daemon=True,
        )
        self._stderr_thread = threading.Thread(
            target=self._reader_loop,
            args=("stderr", self.process.stderr),
            daemon=True,
        )
        self._stdout_thread.start()
        self._stderr_thread.start()

        return True, f"Kernel avviato con comando: {' '.join(cmd)} ({reason})"

    def stop(self) -> tuple[bool, str]:
        if self.process is None or self.process.poll() is not None:
            self.process = None
            return True, "Kernel non in esecuzione."

        self.process.terminate()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=2)

        self.process = None
        return True, "Kernel fermato."

    def is_running(self) -> bool:
        return self.process is not None and self.process.poll() is None

    def _reader_loop(self, source: str, stream):
        if stream is None:
            return
        for line in stream:
            clean = line.rstrip("\n")
            if clean:
                self.events.put(KernelEvent(source=source, line=clean))

    def _resolve_start_command(self) -> tuple[list[str], str]:
        binary = self.workspace_root / "target" / "release" / "agentic_os_kernel"
        if binary.exists() and self._binary_is_fresh(binary):
            return [str(binary)], "release binary up-to-date"
        if binary.exists():
            return ["cargo", "run", "--release"], "release binary stale vs Rust sources"
        return ["cargo", "run", "--release"], "release binary missing"

    def _binary_is_fresh(self, binary: Path) -> bool:
        try:
            binary_mtime = binary.stat().st_mtime
        except OSError:
            return False

        tracked_paths = [self.workspace_root / "Cargo.toml"]
        tracked_paths.extend((self.workspace_root / "src").rglob("*.rs"))

        latest_source_mtime = binary_mtime
        for path in tracked_paths:
            try:
                latest_source_mtime = max(latest_source_mtime, path.stat().st_mtime)
            except OSError:
                continue

        return binary_mtime >= latest_source_mtime
