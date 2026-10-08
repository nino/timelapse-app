import { fileURLToPath } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

import { readChangelog } from "./scripts/changelog.ts";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

const CHANGELOG = "virtual:changelog";

// The About window's change log, read from git once per build (or dev server
// start) and compiled into the bundle, so it needs no network.
function changelog(): Plugin {
  const resolved = `\0${CHANGELOG}`;
  return {
    name: "changelog",
    resolveId(id: string): string | undefined {
      return id === CHANGELOG ? resolved : undefined;
    },
    load(id: string): string | undefined {
      if (id !== resolved) return undefined;
      return `export const changelog = ${JSON.stringify(readChangelog(fileURLToPath(new URL(".", import.meta.url))))};`;
    },
  };
}

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss(), changelog()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
