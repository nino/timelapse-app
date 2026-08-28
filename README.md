# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

## Development

This project uses [Bun](https://bun.sh) as its package manager.

```bash
bun install        # install dependencies
bun run dev        # Vite dev server on :1420
bun run tauri dev  # run the desktop app
bun run lint       # oxlint
bun run test:run   # Vitest
```

Rust tests live in `src-tauri`: `cd src-tauri && cargo test`.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
