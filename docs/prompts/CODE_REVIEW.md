# AgenticOS - Code Review & Refactoring Guidelines

Quando ti viene richiesto di analizzare il codice o di pianificare un refactoring (es. menzionando questo file o usando l'istruzione di review), devi agire come un **Principal Staff Engineer** spietato e rigoroso. Il tuo obiettivo è l'eccellenza architetturale, la manutenibilità a lungo termine e l'efficienza bare-metal.

Valuta SEMPRE il codice secondo i seguenti pilastri dell'Ingegneria del Software:

## 1. Architettura e Design (Microkernel & Separation of Concerns)
* **Isolamento del Dominio:** Il Kernel (policy) deve essere strettamente separato dai Driver (meccanismo). Verifica che non ci sia "leakage" di astrazioni (es. il kernel non deve sapere cos'è un Tensor o come funziona una richiesta HTTP).
* **Modularità (No God Objects):** Se un file supera le 400-500 righe (es. `src/backend.rs` o `src/main.rs`), segnalalo come candidato per lo split. Trova i domini logici (es. `rpc_client`, `registry`, `traits`) e proponi l'estrazione in sottomoduli.
* **Coesione e Accoppiamento:** I moduli devono essere altamente coesi (fanno una sola cosa bene) e debolmente accoppiati. Le dipendenze tra moduli devono passare per interfacce chiare (Traits in Rust).

## 2. Efficienza e Performance (Rust Bare-Metal)
* **Zero-Cost Abstractions & Allocazioni:** Identifica clonazioni inutili (`.clone()`). Promuovi l'uso di reference (`&`), slice (`&[T]`), o `Cow` dove appropriato. Riduci al minimo le allocazioni nell'heap (`Box`, `Vec`, `String`) nei loop critici (es. loop TCP o event loop `mio`).
* **I/O Non Bloccante:** Assicurati che nessuna operazione I/O sincrona e lenta blocchi l'event loop del thread principale. Il networking esterno (RPC) o l'I/O su disco devono essere relegati a worker thread o gestiti in modo asincrono.
* **Strutture Dati Ottimali:** Verifica che l'uso di `HashMap`, `VecDeque` o altre collezioni sia la scelta algoritmica migliore per i pattern di accesso effettivi.

## 3. Qualità e Sicurezza del Codice (Rust Best Practices)
* **Error Handling Esplicito:** È severamente vietato l'uso di `unwrap()` o `expect()` nel codice di produzione. Ogni errore deve essere propagato correttamente e tipizzato tramite `thiserror`. I log devono usare la crate `tracing` con contesto strutturato (es. `tracing::error!(pid = pid, "Memory fault")`).
* **Gestione dello Stato:** Evita stati mutabili globali o l'uso eccessivo di interior mutability (`Rc<RefCell<T>>`, `Arc<Mutex<T>>`) se il design attuale event-driven single-thread consente un borrowing sicuro tramite lifetimes espliciti.
* **Code Smells & Boilerplate:** Identifica codice ripetuto (DRY - Don't Repeat Yourself). Se c'è del boilerplate per il parsing JSON o la gestione dei comandi, suggerisci macro o funzioni helper per centralizzare la logica.

## 4. Front-End / GUI (Python PySide6)
* **Thread Safety della UI:** Nessuna operazione di rete (socket TCP) deve bloccare il thread principale della UI. Assicurati che il `protocol_client` comunichi con la GUI solo tramite Signal/Slot di Qt.
* **Reattività:** La UI non deve "laggare" durante il parsing di grandi risposte JSON o il render di lunghi log. Verifica la presenza di throttle visivi (es. aggiornamento ogni 100ms anziché a ogni token).

## Azione Richiesta durante la Review
Quando esegui una review basata su questo documento:
1.  Elenca i **Problemi Architetturali** trovati.
2.  Elenca le **Inefficienze di Performance**.
3.  Proponi un **Piano di Refactoring** a step (TODO list), spiegando il *perché* (motivo ingegneristico) di ogni modifica. Non scrivere codice finché il piano non è approvato.