---
description: Review the current diff for correctness, security, and scope drift
allowed-tools: Bash(git diff:*), Bash(git status:*), Read, Grep, Glob
---

# Review

Review the pending changes in this repository.

## Gather the diff

```bash
git status --short
git diff HEAD
```

## Review against these dimensions, in priority order

**1. Security (highest — this app has a hostile-peer boundary)**

- Does any new code write to a path derived from remote input? It must go through
  `filesystem::safe_join`. Flag anything that doesn't.
- Are remote-supplied sizes, indices, or counts used to allocate before validation?
- Does anything log file contents or key material?
- Was any cryptographic primitive implemented by hand? That is a hard block,
  not a suggestion — cite SRS §34 / NFR-SEC-009.

**2. Correctness**

- Resume logic: is a partial chunk ever persisted as complete? SRS FR-CHUNK-001
  makes a chunk the atomic unit of resume. Violations are bugs, not style.
- Transfer state machine: can it reach `COMPLETED` without verification passing?
- Error paths that swallow failures or convert an error into success.

**3. IPC boundary consistency**

If a Rust struct crossing the IPC boundary changed, was `src/types/index.ts`
updated in the same diff? Mismatches here fail silently at runtime.

**4. Scope drift**

Does the change implement something in the SRS? If not, say so plainly and stop —
do not review it as if it were in scope. The SRS is the baseline (SRS §113).

## Output

Group findings by severity. For each: file:line, what's wrong, why it matters,
and the concrete failure it causes. If you find nothing at a severity level, say
so — do not pad. Do not restate the diff.
