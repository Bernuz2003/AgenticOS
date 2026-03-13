# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## AgenticOS Notes

- The Tauri workspace now uses the kernel control plane plus `workspace/agenticos.db` as the primary source for sessions, accounting, audit replay, runtime inventory, and timeline history.
- `timeline_sessions/*.json` is deprecated as a GUI dependency. Legacy files may still exist for compatibility or old imports, but the current workspace UI no longer relies on them for normal replay or session recovery.
