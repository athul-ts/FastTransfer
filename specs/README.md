# FastTransfer feature specs

This folder holds the **feature specifications** that drive implementation. It is
how the project does spec-driven development: a feature is specified, validated
and approved here *before* its production code is written, and its acceptance
criteria become the tests that prove it.

## How this relates to the SRS

[`FastTransfer_SRS_v3.0.md`](../FastTransfer_SRS_v3.0.md) is the requirements
baseline and wins every conflict (SRS §114.3). A feature spec:

- **groups** SRS requirements into a buildable feature,
- **validates** each one (is it clear, feasible and testable as written?),
- **decides** what the SRS leaves open, as numbered design decisions,
- **turns** requirements into acceptance criteria with named tests.

A spec never contradicts the SRS. When a spec needs the SRS to change, it raises a
change request (`CR-NN`) in [`validation-report.md`](validation-report.md), and the
SRS is edited under its own rules (§114.4): a revision-history entry, no
renumbered IDs, a traceability update, and a security review for any NFR-SEC-*,
§91 or §92 change.

## Files

| File | Purpose |
|---|---|
| [`validation-report.md`](validation-report.md) | Audit of the whole SRS: document defects, a verdict for every requirement, change requests, and code defects found while checking |
| [`_template.md`](_template.md) | Copy this to start a new feature spec |
| `F01`–`F12` | One spec per feature, below |

## Feature index

| ID | Feature | SRS phase | Verdict | Spec status |
|---|---|---|---|---|
| [F01](F01-cable-link-detection.md) | Cable link detection | 2 | VALID WITH CHANGES | Draft |
| [F02](F02-interfaces-and-transport-selection.md) | Interface enumeration and transport selection | 2 | VALID | Draft |
| [F03](F03-router-free-discovery.md) | Router-free peer discovery | 3 | VALID | Implemented (T); D pending |
| [F04](F04-session-and-wire-protocol.md) | Session establishment and wire protocol | 4 | VALID WITH CHANGES | Approved — In Progress |
| [F05](F05-file-and-folder-transfer.md) | File and folder transfer | 4–5 | VALID | Approved — In Progress |
| [F06](F06-chunking-resume-integrity.md) | Chunking, resume and integrity | 6 | VALID WITH CHANGES | Approved — In Progress |
| [F07](F07-filesystem-safety.md) | Filesystem safety and compatibility | 4–5 | VALID WITH CHANGES | Approved — In Progress |
| [F08](F08-pairing-and-security.md) | Pairing, authentication and encryption | 7 | NEEDS CHANGE (blocking) | Draft |
| [F09](F09-performance-and-limits.md) | Performance, backpressure and resource limits | 8 | CONDITIONAL | Draft |
| [F10](F10-desktop-ui.md) | Desktop UI | 9 | VALID WITH CHANGES | Draft |
| [F11](F11-diagnostics-errors-logging.md) | Diagnostics, errors and logging | 9 | VALID WITH CHANGES | Draft |
| [F12](F12-packaging-ci-release.md) | Packaging, CI and release | 10 | VALID WITH CHANGES | Draft |

**Thunderbolt 3 / USB-C cable transfer** is not a feature of its own. It is F01
(detect the cable honestly) + F02 (prefer the Thunderbolt interface) + F03–F06
(the transfer path, which is identical over every IP interface). Its validity is
discussed in [`validation-report.md` §1](validation-report.md#1-summary).

## Workflow

```text
Draft ──▶ In Review ──▶ Approved ──▶ In Progress ──▶ Implemented ──▶ Verified
  ▲           │
  └─ changes ─┘          (Blocked and Superseded can happen at any point)
```

1. **Draft.** Copy `_template.md`. Fill in every section. List every SRS ID in
   scope and give each a verdict.
2. **In Review.** The owner reads it. Open decisions (`D-Fnn-n`) are accepted,
   changed or rejected. Change requests go into the validation report.
3. **Approved.** No open decision blocks the first slice. Production code for the
   feature may start only now. Fixes to existing defects are exempt.
4. **In Progress.** Tests are written **first**, from the acceptance criteria, and
   are expected to fail. Code is written until they pass.
5. **Implemented.** Every T-method criterion has a passing test. D-method criteria
   (real hardware) may still be pending.
6. **Verified.** Every criterion has a recorded method, procedure and result
   (SRS §117.2). Update the SRS §118 row in the same change.

## Rules

- **Every acceptance criterion has an ID**, `AC-Fnn-mm`, and names its test. A
  test that proves a criterion says so in its doc comment: `/// AC-F06-03`.
  Searching the repo for an AC ID finds its test.
- **Verification methods** are the SRS's: **T** test, **D** demonstration on real
  hardware, **A** analysis, **I** inspection (§117).
- **Hardware-conditional criteria are labelled.** A criterion that needs a pair
  of machines that both expose a Thunderbolt network interface is marked
  `CONDITIONAL (§5.2)`. It is never satisfied by misreporting the link (FR-LINK-003).
- **No silent `should`.** If a spec decides not to implement a *should*, it
  records why (§114.2).
- **Specs track code honestly.** "Implemented" means a passing test exists.
  "Partial" names what is missing. Code that exists but is not yet committed is
  marked as such.

## Verdicts used

| Verdict | Meaning |
|---|---|
| **VALID** | Clear, feasible and testable as written |
| **VALID WITH CHANGES** | Sound, but needs a clarification, metric or decision before it can be built or tested |
| **CONDITIONAL** | Valid, but only verifiable on hardware the project does not have (§5.2, §116.1.1) |
| **NEEDS CHANGE** | Cannot be implemented correctly as written; blocked on a change request |
| **INVALID** | Infeasible or self-contradictory; must be rewritten or withdrawn |

## Implementation status used

| Status | Meaning |
|---|---|
| **Not started** | No code |
| **Partial** | Some code; the spec says what is missing |
| **Implemented** | Code and passing tests, not yet demonstrated where D is required |
| **Verified** | Every method recorded with an observed result |
