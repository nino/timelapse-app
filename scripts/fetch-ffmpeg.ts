#!/usr/bin/env bun

// Downloads the static ffmpeg the app bundles as a Tauri sidecar, into
// src-tauri/binaries/ffmpeg-<target triple> where `externalBin` in
// src-tauri/tauri.macos.conf.json expects it. Runs before `tauri dev` and
// `tauri build`; a no-op when the pinned build is already there, and on
// non-macOS targets, which have no sidecar. For `universal-apple-darwin` it
// fetches both architectures and joins them with `lipo`.
//
// Usage: bun scripts/fetch-ffmpeg.ts [target-triple]

import { createHash } from "crypto";
import { chmod, mkdir, readFile, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import { join } from "path";

// Martin Riedl's static macOS builds of ffmpeg 9.0.2 (https://ffmpeg.martin-riedl.de),
// which include libx265 for the converter's HEVC encode. Changing a pin
// re-downloads on the next run.
const BUILDS: Record<string, { url: string; sha256: string }> = {
  "aarch64-apple-darwin": {
    url: "https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip",
    sha256: "c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924",
  },
  "x86_64-apple-darwin": {
    url: "https://ffmpeg.martin-riedl.de/download/macos/amd64/1789931006_9.0.2/ffmpeg.zip",
    sha256: "7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4",
  },
};

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

// Run a command, treating a missing executable like any other failure: Bun
// throws on ENOENT rather than reporting a failed spawn.
function run(command: string[]): { success: boolean; stdout: Buffer; stderr: string } {
  try {
    const result = Bun.spawnSync(command);
    return { success: result.success, stdout: result.stdout, stderr: result.stderr.toString() };
  } catch (error) {
    return { success: false, stdout: Buffer.alloc(0), stderr: String(error) };
  }
}

function hostTriple(): string {
  const result = run(["rustc", "-vV"]);
  const host = result.success ? result.stdout.toString().match(/^host: (\S+)$/m) : null;
  if (!host) {
    fail("Could not work out the target triple from `rustc -vV`; pass it as an argument.");
  }
  return host[1];
}

const triple = process.argv[2] ?? process.env.TAURI_ENV_TARGET_TRIPLE ?? hostTriple();

if (!triple.endsWith("-apple-darwin")) {
  process.exit(0);
}

// Relative to this script, so it works from any working directory.
const binariesDir = join(import.meta.dir, "..", "src-tauri", "binaries");

// Install the pinned ffmpeg for one architecture at `binary`, unless the
// marker beside it says that exact build is already there.
async function installSingle(arch: string, binary: string): Promise<void> {
  const build = BUILDS[arch];
  if (!build) {
    fail(`No pinned ffmpeg build for ${arch}.`);
  }
  // The hash of the zip the binary came from, so a changed pin is noticed.
  const marker = `${binary}.sha256`;
  const installed = await readFile(marker, "utf8").catch((): string => "");
  if (installed.trim() === build.sha256 && (await Bun.file(binary).exists())) {
    return;
  }

  console.log(`Downloading ffmpeg for ${arch} from ${build.url}`);
  const response = await fetch(build.url);
  if (!response.ok) {
    fail(`Download failed: ${response.status} ${response.statusText}`);
  }
  const zip = new Uint8Array(await response.arrayBuffer());

  const actual = createHash("sha256").update(zip).digest("hex");
  if (actual !== build.sha256) {
    fail(`Checksum mismatch for ${build.url}: expected ${build.sha256}, got ${actual}`);
  }

  const zipPath = join(tmpdir(), `ffmpeg-${arch}-${actual}.zip`);
  await writeFile(zipPath, zip);
  const unzip = run(["unzip", "-p", zipPath, "ffmpeg"]);
  await rm(zipPath, { force: true });
  if (!unzip.success || unzip.stdout.length === 0) {
    fail(`Could not unpack ffmpeg: ${unzip.stderr}`);
  }

  await writeFile(binary, unzip.stdout);
  await chmod(binary, 0o755);
  await writeFile(marker, `${build.sha256}\n`);
  console.log(`Installed ${binary}`);
}

await mkdir(binariesDir, { recursive: true });
const binaryFor = (arch: string): string => join(binariesDir, `ffmpeg-${arch}`);

if (triple === "universal-apple-darwin") {
  // Tauri wants one fat sidecar for a universal bundle.
  const arches = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  for (const arch of arches) {
    await installSingle(arch, binaryFor(arch));
  }
  const universal = binaryFor(triple);
  const marker = `${universal}.sha256`;
  const expected = arches.map((arch): string => BUILDS[arch].sha256).join("+");
  const installed = await readFile(marker, "utf8").catch((): string => "");
  if (installed.trim() !== expected || !(await Bun.file(universal).exists())) {
    const lipo = run(["lipo", "-create", "-output", universal, ...arches.map(binaryFor)]);
    if (!lipo.success) {
      fail(`Could not build a universal ffmpeg with lipo: ${lipo.stderr}`);
    }
    await writeFile(marker, `${expected}\n`);
    console.log(`Installed ${universal}`);
  }
} else {
  await installSingle(triple, binaryFor(triple));
}
