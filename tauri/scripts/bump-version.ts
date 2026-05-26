#!/usr/bin/env node

import { readFile, writeFile } from "fs/promises";
import { resolve } from "path";

type VersionType = "major" | "minor" | "patch";

const versionType = (process.argv[2] ?? "patch") as VersionType;

if (!["major", "minor", "patch"].includes(versionType)) {
  console.error("Usage: node bump-version.ts [major|minor|patch]");
  process.exit(1);
}

function incrementVersion(version: string, type: VersionType): string {
  const parts = version.split(".").map(Number);

  if (type === "major") {
    parts[0] += 1;
    parts[1] = 0;
    parts[2] = 0;
  } else if (type === "minor") {
    parts[1] += 1;
    parts[2] = 0;
  } else {
    parts[2] += 1;
  }

  return parts.join(".");
}

// Update package.json
const packageJsonPath = resolve("package.json");
const packageJson = JSON.parse(await readFile(packageJsonPath, "utf8"));
const oldVersion = packageJson.version;
const newVersion = incrementVersion(oldVersion, versionType);
packageJson.version = newVersion;
await writeFile(packageJsonPath, JSON.stringify(packageJson, null, 2) + "\n");

// Update tauri.conf.json
const tauriConfPath = resolve("src-tauri/tauri.conf.json");
const tauriConf = JSON.parse(await readFile(tauriConfPath, "utf8"));
tauriConf.version = newVersion;
await writeFile(tauriConfPath, JSON.stringify(tauriConf, null, 2) + "\n");

console.log(`Version bumped from ${oldVersion} to ${newVersion}`);
