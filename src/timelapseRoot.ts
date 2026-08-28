/**
 * Name of the directory under `$HOME` holding screenshots, rendered videos and
 * the extracted-frame cache.
 *
 * Rust owns this value — `TIMELAPSE_DIR_NAME` in `src-tauri/src/paths.rs`
 * switches it to `Timelapse_dev` for debug builds so `bun run tauri dev` never
 * reads from or writes into the real library. The frontend deliberately does
 * *not* re-derive it from `import.meta.env.DEV`: that is a different axis from
 * Rust's `cfg!(debug_assertions)`, and the two disagree under `tauri dev
 * --release` and `tauri build --debug`, which would silently point the capture
 * loop at one library and the viewer at the other.
 */
let root: string | null = null;

/**
 * Resolve the root from Rust. Must run before the first render.
 *
 * `invoke` is imported lazily so that merely importing this module does not
 * pull the Tauri bridge into the graph — which keeps it usable from tests.
 */
export async function initTimelapseRoot(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  root = await invoke<string>("get_timelapse_root_name");
}

/**
 * The resolved root. Throws rather than guessing — falling back to
 * `"Timelapse"` would mean a dev build quietly writing into the real library.
 */
export function timelapseRoot(): string {
  if (root === null) {
    throw new Error(
      "timelapseRoot() used before initTimelapseRoot() resolved the root",
    );
  }
  return root;
}

/** Test seam: set the root without going through Tauri. */
export function setTimelapseRootForTests(name: string | null): void {
  root = name;
}
