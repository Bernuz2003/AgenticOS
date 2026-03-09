# client.py

import os
import socket
import sys
from pathlib import Path

_REPO_ROOT = Path(__file__).resolve().parents[1]
if str(_REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(_REPO_ROOT))

from agenticos_shared.runtime_config import load_runtime_defaults


def _load_runtime_config():
    runtime = load_runtime_defaults(
        {
            "host": "127.0.0.1",
            "port": int(os.environ.get("AGENTIC_PORT", "6380")),
        }
    )
    return runtime["host"], runtime["port"]

def send_command(verb, agent_id, payload):
    payload_bytes = payload.encode('utf-8')
    header = f"{verb} {agent_id} {len(payload_bytes)}\n"
    
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            host, port = _load_runtime_config()
            s.connect((host, port))
            s.sendall(header.encode('utf-8'))
            s.sendall(payload_bytes)
            
            # Loop di lettura continua
            while True:
                data = s.recv(1024) # Leggi a chunk
                if not data:
                    break
                # Stampa live senza andare a capo (streaming)
                print(data.decode('utf-8', errors='replace'), end='', flush=True)
            print() # A capo finale quando chiude
            
    except ConnectionRefusedError:
        print("Errore: Kernel offline")
    except KeyboardInterrupt:
        print("\nDisconnesso.")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Uso: python3 client.py <VERB> <PAYLOAD>")
        sys.exit(1)
    send_command(sys.argv[1], "sys", sys.argv[2])