# Protocol Contracts

This directory contains versioned control-plane contract artifacts.

Rules for evolution:
- Keep TCP framing unchanged.
- Additive changes inside an existing schema version are allowed.
- Breaking semantic changes require a new schema ID and protocol version.
- Clients should negotiate protocol support with `HELLO` before assuming envelope semantics.
- Legacy payload mode remains available while `allow_legacy_fallback` is enabled.
