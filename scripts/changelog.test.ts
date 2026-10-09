import { describe, expect, it } from 'bun:test';

import { commitCounts, noticeable, parseLog, pullRequest, versions, type MainCommit } from './changelog';

function commit(subject: string, body = '', files = ['src/App.tsx']): MainCommit {
  return { sha: 'a', date: '2026-10-08', subject, body, files };
}

describe('pullRequest', () => {
  it('takes a merge commit’s title from its body', () => {
    expect(pullRequest(commit('Merge pull request #155 from nino/branch', '\nCapture raw pixels\n\nMore'))).toEqual({
      pr: 155,
      title: 'Capture raw pixels',
    });
  });

  it('takes a squash merge’s title from its subject', () => {
    expect(pullRequest(commit('Size the Settings window to its content (#142)'))).toEqual({
      pr: 142,
      title: 'Size the Settings window to its content',
    });
  });

  it('skips commits pushed straight to main', () => {
    expect(pullRequest(commit('Version bump'))).toBeNull();
    expect(pullRequest(commit("Merge branch 'main'"))).toBeNull();
  });
});

describe('noticeable', () => {
  it('leaves out PRs that only touched docs, CI or tests', () => {
    expect(noticeable(commit('Tidy (#1)', '', ['CLAUDE.md', '.github/workflows/ci.yml', 'src/App.test.tsx']))).toBe(false);
    expect(noticeable(commit('Fix (#2)', '', ['CLAUDE.md', 'src/App.tsx']))).toBe(true);
  });

  it('leaves out dependency bumps and titles made from branch names', () => {
    expect(noticeable(commit('Bump vite from 8.1 to 8.2 (#3)'))).toBe(false);
    expect(noticeable(commit('Merge pull request #4 from nino/x', 'claude/session-0123'))).toBe(false);
  });
});

describe('parseLog', () => {
  it('reads each commit’s fields and files', () => {
    const log =
      '\x1eabc\x1f2026-10-08\x1fMerge pull request #1 from x\x1fTitle\n\x1d\n\nsrc/a.ts\nsrc/b.ts\n' +
      '\x1edef\x1f2026-10-07\x1fFix (#2)\x1f\x1d\n\nsrc/c.ts\n';
    expect(parseLog(log)).toEqual([
      { sha: 'abc', date: '2026-10-08', subject: 'Merge pull request #1 from x', body: 'Title\n', files: ['src/a.ts', 'src/b.ts'] },
      { sha: 'def', date: '2026-10-07', subject: 'Fix (#2)', body: '', files: ['src/c.ts'] },
    ]);
  });
});

describe('commitCounts', () => {
  it('counts each commit’s ancestors, merged branches included, like git rev-list --count', () => {
    // a ← b ← m (merges c, whose parent is a) ← d
    const revList = ['d m', 'm b c', 'c a', 'b a', 'a'].join('\n');
    const counts = commitCounts(revList, ['d', 'm', 'b', 'a']);
    expect(Object.fromEntries(counts)).toEqual({ a: 1, b: 2, m: 4, d: 5 });
  });
});

describe('versions', () => {
  it('numbers commits before 0.2.0 by commit count and later ones by the release that shipped them', () => {
    const numbered = [
      { config: '0.1.3', count: 198 },
      { config: '0.1.3', count: 200 },
      { config: '0.2.0', count: 203, tag: '0.2.0' },
      // Superseded before its release published: shipped in the next one.
      { config: '0.2.0', count: 205 },
      { config: '0.2.0', count: 207, tag: '0.2.1' },
      // Not released yet.
      { config: '0.2.0', count: 209 },
    ];
    expect(versions(numbered, '0.2.2')).toEqual(['0.1.198', '0.1.200', '0.2.0', '0.2.1', '0.2.1', '0.2.2']);
  });
});
