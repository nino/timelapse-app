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

`bun run tauri dev` keeps its screenshots, videos and database in
`~/Timelapse_dev`, so development never touches the real `~/Timelapse` library
that `bun run tauri build` produces. The Rust build decides which of the two is
in use and the frontend asks it at startup, so the capture loop and the viewer
cannot disagree.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
