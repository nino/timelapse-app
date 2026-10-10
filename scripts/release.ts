#!/usr/bin/env bun
// What release.yml asks of git about the release it is building.
//
//   bun scripts/release.ts version          the version a stable build of HEAD gets
//   bun scripts/release.ts version --beta   the stable version a beta of HEAD leads up to
//   bun scripts/release.ts latest <version> whether no released version is higher (true/false)
//   bun scripts/release.ts notes <version>  the release notes: the change log's entries for it

import { readFile } from "node:fs/promises";

import { readChangelog } from "./changelog.ts";
import { compareVersions, nextVersion, readTags } from "./version.ts";

const cwd = process.cwd();
const [command, argument] = process.argv.slice(2);

if (command === "version") {
  const tags = readTags(cwd);
  const head = (await Bun.$`git rev-parse HEAD`.cwd(cwd).text()).trim();
  const config = String(JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8")).version);
  // Rebuilding a released commit keeps its version, so the release is replaced
  // rather than a second number spent on the same code. A beta always leads up
  // to a version not released yet, so it sorts above every stable release.
  const tagged = argument === "--beta" ? undefined : tags.get(head);
  console.log(tagged ?? nextVersion(config, [...tags.values()]));
} else if (command === "latest" && argument) {
  // A re-run on an older released commit rebuilds its version, which must not
  // take GitHub's "latest" (where the updater looks) from a newer one.
  const released = [...readTags(cwd, true).values()];
  console.log(released.every((version) => compareVersions(version, argument) <= 0));
} else if (command === "notes" && argument) {
  const entries = readChangelog(cwd).filter((entry) => entry.version === argument);
  console.log(
    entries.length === 0
      ? "No changes you would notice in the app."
      : entries.map((entry) => `- ${entry.title} (#${entry.pr})`).join("\n"),
  );
} else {
  console.error("Usage: bun scripts/release.ts version [--beta] | latest <version> | notes <version>");
  process.exit(1);
}
