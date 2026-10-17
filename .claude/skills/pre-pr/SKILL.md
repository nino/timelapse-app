---
name: pre-pr
description: Review and fix your own changes before opening or marking ready a pull request in this repo. Use before every `create_pull_request`, `gh pr create` or push that opens a PR.
---

# Before you open a PR

Run these steps in order on the branch you are about to publish. Don't open the PR until every step is clean; fix what they find and go round again.

## 1. See the whole diff

```sh
git fetch origin main
git diff origin/main...HEAD --stat
git diff origin/main...HEAD
```

Include uncommitted work (`git status`). Every file in the diff should belong to the change; drop stray edits, debug output and generated files (`dist/`, `src-tauri/binaries/`, `package-lock.json`).

## 2. Review it

Run the built-in `/code-review` skill on the branch at `high` effort with `--fix`, then `/simplify`. If those skills aren't available, review the diff yourself for correctness bugs first, then for code that duplicates something the repo already has or could be shorter.

Then check the diff against this repo's rules, which a generic review won't know:

- **No effects** outside the allowlisted hooks in `src/hooks/` (see `.oxlintrc.json`). Derive in render, act in the handler, or build on those hooks.
- **Performance comes first**: nothing that costs resize or scrub smoothness in the UI, or battery in background work. No blurred shadows or squircle corners.
- **Data safety**: PNGs are deleted only after both video conversion and OCR say an hour is done; frames are keyed on `(day, frame_number)`.
- **Library root**: never hardcode `Timelapse`/`Timelapse_dev` or derive it from `import.meta.env.DEV`.
- **Self-contained**: no Homebrew or system tools, no `ffprobe`; ffmpeg goes through `paths::ffmpeg()`.
- **TypeScript**: explicit return types, no `any`, named exports, erasable syntax only (no enums, namespaces or parameter properties).
- **Version**: the PR doesn't touch the version in `package.json` or `tauri.conf.json`.
- **Docs**: if behaviour changed, the CLAUDE.md for that folder (root, `src/`, `src-tauri/`) or the module's doc comment says so, and nothing there is now wrong.
- **Tests** cover the new behaviour, and no test was skipped or deleted to get green.

Fix every finding that holds up. For one you decide not to fix, note why; it goes in the PR description.

## 3. Run the checks CI runs

```sh
bun run lint
bun run build
bun test
```

If anything under `src-tauri/` changed, also run `cd src-tauri && cargo test --workspace`; it works in cloud sessions. All must pass with no new lint warnings.

## 4. Open the PR

- **Title** in plain language a user understands: the change log and release notes are built from PR titles.
- **Description** says what changed and why, how you tested it, and any review findings you chose not to fix.
- After pushing review fixes, re-run step 3 before you call the PR ready.
