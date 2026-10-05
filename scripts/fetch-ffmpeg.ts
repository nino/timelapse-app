#!/usr/bin/env bun

// Downloads the static ffmpeg the app bundles as a Tauri sidecar, into
// src-tauri/binaries/ffmpeg-<target triple> where `externalBin` in
// src-tauri/tauri.macos.conf.json expects it. Runs before `tauri dev` and
// `tauri build`; a no-op when the pinned build is already there, and on
// non-macOS targets, which have no sidecar.
//
// Usage: bun scripts/fetch-ffmpeg.ts [target-triple]

import { createHash } from "crypto";
import { chmod, mkdir, readFile, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import { join, resolve } from "path";

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

function hostTriple(): string {
  const result = Bun.spawnSync(["rustc", "-vV"]);
  const host = result.stdout.toString().match(/^host: (\S+)$/m);
  if (!result.success || !host) {
    console.error("Could not work out the target triple from `rustc -vV`; pass it as an argument.");
    process.exit(1);
  }
  return host[1];
}

const triple = process.argv[2] ?? process.env.TAURI_ENV_TARGET_TRIPLE ?? hostTriple();

if (!triple.endsWith("-apple-darwin")) {
  process.exit(0);
}

const build = BUILDS[triple];
if (!build) {
  console.error(`No pinned ffmpeg build for ${triple}.`);
  process.exit(1);
}

const binariesDir = resolve("src-tauri/binaries");
const binary = join(binariesDir, `ffmpeg-${triple}`);
// The hash of the zip the binary came from, so a changed pin is noticed.
const marker = `${binary}.sha256`;

const installed = await readFile(marker, "utf8").catch((): string => "");
if (installed.trim() === build.sha256 && (await Bun.file(binary).exists())) {
  process.exit(0);
}

console.log(`Downloading ffmpeg for ${triple} from ${build.url}`);
const response = await fetch(build.url);
if (!response.ok) {
  console.error(`Download failed: ${response.status} ${response.statusText}`);
  process.exit(1);
}
const zip = new Uint8Array(await response.arrayBuffer());

const actual = createHash("sha256").update(zip).digest("hex");
if (actual !== build.sha256) {
  console.error(`Checksum mismatch for ${build.url}: expected ${build.sha256}, got ${actual}`);
  process.exit(1);
}

const zipPath = join(tmpdir(), `ffmpeg-${triple}-${actual}.zip`);
await writeFile(zipPath, zip);
await mkdir(binariesDir, { recursive: true });
const unzip = Bun.spawnSync(["unzip", "-p", zipPath, "ffmpeg"]);
await rm(zipPath, { force: true });
if (!unzip.success || unzip.stdout.length === 0) {
  console.error(`Could not unpack ffmpeg: ${unzip.stderr.toString()}`);
  process.exit(1);
}

await writeFile(binary, unzip.stdout);
await chmod(binary, 0o755);
await writeFile(marker, `${build.sha256}\n`);
console.log(`Installed ${binary}`);
