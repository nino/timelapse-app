// `bun test` is Bun's built-in test runner, not a package.json script. This
// project's tests are written for Vitest (globals, `vi.mocked`, happy-dom, the
// setup file in vitest.config.ts), so Bun's runner fails almost every one of
// them. bunfig.toml preloads this file into Bun's runner; it runs the whole
// suite under Vitest instead and exits with Vitest's status before Bun runs a
// single test.
//
// Bun does not pass its own CLI arguments to a preload, so file filters and
// `-t` are ignored here. Use `bun run test:run <args>` to pass them to Vitest.
console.error("bun test: this project uses Vitest; running `bun run test:run`.\n");
const result = Bun.spawnSync(["bun", "run", "test:run"], {
  stdio: ["inherit", "inherit", "inherit"],
});
process.exit(result.exitCode ?? 1);
