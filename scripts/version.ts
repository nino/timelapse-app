// Release version numbers.
//
// From 0.2.0 on, every stable release is tagged `v<version>` on main, and the
// next one is the highest such tag reachable from HEAD with its patch number
// plus one. Nothing in a pull request names its version, so PRs merged in
// parallel never conflict over it or claim the same number. `version` in
// tauri.conf.json is a floor: `bun run version:bump minor` sets it to the next
// minor's `.0`, which then wins over the tags. Releases before 0.2.0 were
// numbered `<major>.<minor>.<commit count>` and have no tags.
//
// Runs under Node (changelog.ts is imported by vite.config.ts), so no Bun APIs
// here.

import { execFileSync } from "node:child_process";

/** The first version numbered from tags; earlier ones were commit counts. */
export const FIRST_TAGGED = "0.2.0";

/** `[major, minor, patch]` of "1.2.3" or "v1.2.3"; `null` for anything else. */
export function parseVersion(text: string): [number, number, number] | null {
  const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(text.trim());
  return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null;
}

/** Negative, zero or positive as `a` sorts below, with or above `b`. */
export function compareVersions(a: string, b: string): number {
  const [x, y] = [parseVersion(a), parseVersion(b)];
  if (!x || !y) throw new Error(`not a version: ${x ? b : a}`);
  return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
}

/** Whether a commit whose tauri.conf.json said `config` was numbered by commit count. */
export function isLegacy(config: string): boolean {
  return parseVersion(config) === null || compareVersions(config, FIRST_TAGGED) < 0;
}

/**
 * The version the next release from HEAD gets: one patch above the highest
 * released version, or `config` (tauri.conf.json's) when that is higher or
 * nothing has been released yet.
 */
export function nextVersion(config: string, released: string[]): string {
  const highest = [...released].sort(compareVersions).at(-1);
  if (!highest) return config;
  const [major, minor, patch] = parseVersion(highest) as [number, number, number];
  const next = `${major}.${minor}.${patch + 1}`;
  return compareVersions(config, next) > 0 ? config : next;
}

/**
 * The `v<version>` tags as a map from the commit each points at to its
 * version. `refs` is `git for-each-ref` run with `TAG_FORMAT`. A commit with
 * two tags keeps the higher one.
 */
export function parseTags(refs: string): Map<string, string> {
  const tags = new Map<string, string>();
  for (const line of refs.split("\n")) {
    // An annotated tag names its commit in the peeled field; a lightweight one,
    // which is what `gh release create` makes, only in the object field.
    const [name, peeled, object] = line.trim().split(" ");
    const sha = peeled || object;
    if (!name || !sha || !parseVersion(name)) continue;
    const version = name.replace(/^v/, "");
    const known = tags.get(sha);
    if (!known || compareVersions(version, known) > 0) tags.set(sha, version);
  }
  return tags;
}

export const TAG_FORMAT = "%(refname:short) %(*objectname) %(objectname)";

function git(cwd: string, args: string[]): string {
  return execFileSync("git", args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
}

/** The released versions' tags reachable from HEAD, by commit. */
export function readTags(cwd: string): Map<string, string> {
  return parseTags(git(cwd, ["for-each-ref", "--merged", "HEAD", `--format=${TAG_FORMAT}`, "refs/tags/v*"]));
}
