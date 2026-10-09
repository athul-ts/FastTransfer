# plugins/

Optional [Claude Code plugins](https://docs.claude.com/en/docs/claude-code/plugins) that
contributors can install for this repository. Nothing here is required to build or run
FastTransfer — the app has no runtime dependency on this directory.

Each subdirectory is a self-contained plugin with its own
`.claude-plugin/plugin.json` manifest. Install one with:

```bash
claude plugin install ./plugins/<name>
```

## What belongs here

Plugins that encode knowledge specific to *this* project — a cable diagnostic
workflow, a release checklist, a protocol-review skill. Anything generic enough to
be useful in any repository does not belong in this repo; publish it separately.

## What does not belong here

- **Secrets or credentials.** This directory is committed.
- **Anything that writes network configuration.** The cable diagnostic below is
  read-only on purpose: an automated tool that reconfigures a user's interfaces is
  a support burden and a safety risk.
- **A second copy of a `CLAUDE.md` rule.** Put shared conventions in the root
  `CLAUDE.md` so they apply whether or not a plugin is installed.

## Contents

| Plugin | Purpose |
| --- | --- |
| `cable-diagnostics` | Runs `scripts/verify-cable.sh` and interprets the output |
