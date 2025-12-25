# client.py

import socket
import sys

def send_command(verb, agent_id, payload):
    payload_bytes = payload.encode('utf-8')
    header = f"{verb} {agent_id} {len(payload_bytes)}\n"
    
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.connect(("127.0.0.1", 6379))
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