// The change log the About window shows, read from git at build time.
//
// Every push to main is released (release.yml), as version
// `<major>.<minor>.<commit count>`, so each commit on main's first-parent line
// is a version. Each one that came from a pull request becomes an entry titled
// with the PR's title; commits pushed straight to main, and PRs that only
// touched docs, CI or tests, are left out, since nobody using the app would
// notice them.
//
// Runs under Node (vite.config.ts imports it), so no Bun APIs here.

import { execFileSync } from "node:child_process";

import type { ChangelogEntry } from "../src/about.ts";

export type { ChangelogEntry };

const CONFIG = "src-tauri/tauri.conf.json";

// Field, record and file-list separators in the `git log` output.
const FIELD = "\x1f";
const RECORD = "\x1e";
const FILES = "\x1d";

/** One commit on main's first-parent line, newest first. */
export type MainCommit = {
  sha: string;
  date: string;
  subject: string;
  body: string;
  files: string[];
};

/** Parses `git log` run with `LOG_FORMAT` and `--name-only`. */
export function parseLog(output: string): MainCommit[] {
  return output
    .split(RECORD)
    .filter((record) => record.trim() !== "")
    .map((record) => {
      const [fields, names = ""] = record.split(FILES);
      const [sha, date, subject, body = ""] = fields.split(FIELD);
      return {
        sha: sha.trim(),
        date,
        subject,
        body,
        files: names.split("\n").filter((name) => name !== ""),
      };
    });
}

const LOG_FORMAT = `${RECORD}%H${FIELD}%ad${FIELD}%s${FIELD}%b${FILES}`;

/**
 * The PR a commit on main came from, and its title: a merge commit says
 * "Merge pull request #N from …" with the title as the body's first line; a
 * squash merge is titled "<title> (#N)". `null` for anything else.
 */
export function pullRequest(commit: Pick<MainCommit, "subject" | "body">): { pr: number; title: string } | null {
  const merge = /^Merge pull request #(\d+) /.exec(commit.subject);
  if (merge) {
    const title = commit.body.split("\n").find((line) => line.trim() !== "")?.trim();
    return title ? { pr: Number(merge[1]), title } : null;
  }
  const squash = /^(.*\S)\s+\(#(\d+)\)$/.exec(commit.subject.trim());
  return squash ? { pr: Number(squash[2]), title: squash[1] } : null;
}

/** Files that change nothing in the app someone runs. */
function unseen(file: string): boolean {
  return (
    file.startsWith(".github/") ||
    file.startsWith(".claude/") ||
    file.startsWith("src/test/") ||
    file.endsWith(".md") ||
    /\.test\.tsx?$/.test(file) ||
    file === "bun.lock" ||
    file === "src-tauri/Cargo.lock"
  );
}

/** Whether a commit is worth an entry: it came from a PR and changed the app. */
export function noticeable(commit: MainCommit): boolean {
  const title = pullRequest(commit)?.title ?? "";
  // Dependency bumps, and PRs whose title GitHub made from the branch name.
  if (/^(bump |claude\/)/i.test(title)) return false;
  return commit.files.length === 0 || !commit.files.every(unseen);
}

/**
 * How many commits each first-parent commit has, itself included, which is
 * what `git rev-list --count <sha>` says and what release.yml puts in the
 * version. `revList` is `git rev-list --parents HEAD`; `line` is the first-parent
 * line, newest first.
 */
export function commitCounts(revList: string, line: string[]): Map<string, number> {
  const parents = new Map<string, string[]>();
  for (const row of revList.split("\n")) {
    const [sha, ...rest] = row.trim().split(/\s+/);
    if (sha) parents.set(sha, rest);
  }
  // Walking the line oldest first, everything below the previous commit has
  // already been seen, so each step only visits what its merge brought in.
  const seen = new Set<string>();
  const counts = new Map<string, number>();
  for (const sha of [...line].reverse()) {
    const stack = [sha];
    while (stack.length > 0) {
      const next = stack.pop() as string;
      if (seen.has(next)) continue;
      seen.add(next);
      stack.push(...(parents.get(next) ?? []));
    }
    counts.set(sha, seen.size);
  }
  return counts;
}

function git(cwd: string, args: string[]): string {
  return execFileSync("git", args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
}

function majorMinor(config: string): string {
  const version = String(JSON.parse(config).version ?? "0.0.0");
  return version.split(".").slice(0, 2).join(".");
}

/**
 * The change log for the commit checked out in `cwd`, newest first. Empty
 * outside a git checkout, so a build from a source archive still works.
 */
export function readChangelog(cwd: string): ChangelogEntry[] {
  let commits: MainCommit[];
  let counts: Map<string, number>;
  try {
    commits = parseLog(
      git(cwd, ["log", "--first-parent", "--date=short", `--format=${LOG_FORMAT}`, "--name-only", "HEAD"]),
    );
    counts = commitCounts(
      git(cwd, ["rev-list", "--parents", "HEAD"]),
      commits.map((c) => c.sha),
    );
  } catch (error) {
    console.warn(`No change log: could not read git history (${String(error)})`);
    return [];
  }

  // Major and minor as tauri.conf.json had them at each commit, so a later
  // `version:bump minor` doesn't renumber what was released before it.
  const bumps = new Map<string, string>();
  try {
    for (const sha of git(cwd, ["log", "--first-parent", "--format=%H", "HEAD", "--", CONFIG]).split("\n")) {
      if (sha.trim() === "") continue;
      try {
        bumps.set(sha.trim(), majorMinor(git(cwd, ["show", `${sha.trim()}:${CONFIG}`])));
      } catch {
        // The file was deleted or moved in that commit.
      }
    }
  } catch {
    // Falls back to "0.0" below.
  }

  // Oldest first, carrying the version forward from each change to the config.
  let current = "0.0";
  const entries: ChangelogEntry[] = [];
  for (const commit of [...commits].reverse()) {
    current = bumps.get(commit.sha) ?? current;
    const pr = pullRequest(commit);
    if (!pr || !noticeable(commit)) continue;
    entries.push({ version: `${current}.${counts.get(commit.sha) ?? 0}`, date: commit.date, ...pr });
  }
  return entries.reverse();
}
