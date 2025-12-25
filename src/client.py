import socket
import sys

def send_command(verb, agent_id, payload):
    # 1. Prepara il payload e calcola la lunghezza
    payload_bytes = payload.encode('utf-8')
    length = len(payload_bytes)
    
    # 2. Costruisci l'header
    header = f"{verb} {agent_id} {length}\n"
    
    # 3. Connetti e invia
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.connect(("127.0.0.1", 6379))
            
            # Invia Header + Payload
            s.sendall(header.encode('utf-8'))
            s.sendall(payload_bytes)
            
            # Ricevi risposta
            response = s.recv(4096)
            print("Response:", response.decode('utf-8', errors='replace'))
    except ConnectionRefusedError:
        print("Errore: Il Kernel AgenticOS non è attivo su 127.0.0.1:6379")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Uso: python3 client.py <VERB> <PAYLOAD> [AGENT_ID]")
        print("Esempio: python3 client.py LOAD tinyllama.gguf")
        print("Esempio: python3 client.py EXEC 'Ciao come stai?'")
        sys.exit(1)

    verb = sys.argv[1]
    payload = sys.argv[2]
    agent_id = sys.argv[3] if len(sys.argv) > 3 else "sys"

    send_command(verb, agent_id, payload)