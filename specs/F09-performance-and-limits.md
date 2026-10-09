# F09 — Performance, backpressure and resource limits

| | |
|---|---|
| **Status** | Draft |
| **SRS phase** | 8 (§108) |
| **SRS sources** | §28 (FR-PAR-001), §29, §30 (NFR-PERF-001…007), §31, §32, §33, §60–§65, §70, §93, §101, R-09 |
| **Depends on** | F05, F06 |
| **Verdict** | CONDITIONAL |
| **Last validated** | 2026-11-09 |

## 1. Intent

Move data as fast as the slowest of link, storage and CPU allows, with memory that
does not grow with file size, and report only what was measured.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| NFR-PERF-001 | Use available bandwidth efficiently | shall | **VALID WITH CHANGES** — no metric | Not started |
| NFR-PERF-002 | Avoid unnecessary memory copies | shall | **VALID WITH CHANGES** — no metric | Not started |
| NFR-PERF-003 | Stream large files | shall | VALID (same as FR-TRANSFER-005) | Not started |
| NFR-PERF-004 | Report measured throughput | shall | VALID | Partial: `ThroughputEstimator` |
| NFR-PERF-005 | Multi-GB/s where hardware allows | should | **CONDITIONAL** | Not started |
| NFR-PERF-006 | No fixed low speed ceiling | shall | VALID | Holds by inspection (no throttling code) |
| NFR-PERF-007 | Saturate a Thunderbolt link up to storage limits | shall | **CONDITIONAL** and needs a metric | Not started |
| FR-PAR-001 | Hard cap: 4 streams per transfer, 16 total | shall | VALID | Partial: constants only |
| §29 | Sender slows when receiver cannot keep up | must | VALID | Partial: bounded `pipeline()`; not wired |
| §32 | Memory independent of file size; bounded, configurable buffer pools | must / shall | VALID | Partial: fixed bound (8 × 4 MiB) |
| §33, §60 | No blind compression | shall | VALID | Holds: compression is off |
| §64 | Small-file optimisation | shall | **VALID WITH CHANGES** — conflicts with §100 | Not started |
| §65 | ETA from a moving window | shall | VALID | Implemented (`ThroughputEstimator`) |

## 3. Validity findings

1. **Three performance requirements have no pass mark.** "Efficiently"
   (NFR-PERF-001), "unnecessary copies" (-002) and "saturate" (-007) cannot be
   tested as written. → **CR-12**. Proposed bar for -007: sustained throughput ≥ 80 %
   of min(raw TCP throughput on that link, destination write speed), both measured
   by the benchmark mode (§62) on the same machines.
2. **§64 is "shall" for v1; §100 schedules small-file optimisation for v1.2.**
   → **CR-10**.
3. **SHA-256 may become the bottleneck on Thunderbolt.** SHA-256 with CPU
   acceleration runs at roughly 1.5–2 GB/s per core. D-F06-3 hashes every chunk
   on both sides and re-reads every file on the receiver. On a link that moves
   2–3 GB/s, §23 (SHA-256 shall) and NFR-PERF-007 (saturate) can conflict.
   Needs analysis (method A) before Phase 8; options are hashing on several
   threads, or hashing while writing instead of re-reading. Not measurable on the
   reference hardware.
4. **The reference hardware has no fast link on record.** §116.1.1 lists the
   Linux endpoint's onboard `enp0s31f6` (an Intel I219-class port, typically
   1 Gb/s) and Wi-Fi, and no Ethernet port on the Windows laptop, whose USB-C
   ports are 5 Gb/s USB 3.2 Gen 1 — so a USB Ethernet adapter may be needed for a
   direct cable. Expect measurements around 1 Gb/s at best; multi-GB/s targets
   are unverifiable here (R-09).

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F09-1 | Streams in v1 | One TCP stream per transfer. Add parallel streams (within FR-PAR-001 caps) only after a measurement shows one stream cannot fill the link. | Proposed |
| D-F09-2 | Memory budget | 8 chunks in flight × 4 MiB = 32 MiB per stream, each side. "Configurable" (§32) = the pipeline depth in advanced settings, bounded 2–32. | Proposed |
| D-F09-3 | Benchmark mode (§62) | Writes only to a temporary folder it creates and deletes; labelled "Benchmark"; reports link rating and measured rate side by side. | Proposed |
| D-F09-4 | Compression | Off in v1 (§60 makes it optional). | Proposed |
| D-F09-5 | Small files | v1: measure 100 000 × 4 KiB; set the target from the result. Batching work follows CR-10. | Proposed |
| D-F09-6 | QUIC (§18.2) | Evaluate only after the TCP baseline is measured on a real link. | Proposed |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F09-01 | ETA uses a moving window; a stalled last second does not zero it; no rate is shown before two samples | §65 | T | `transfer::tests::estimator_uses_the_window_not_the_last_instant`, `estimator_needs_two_samples`, `eta_is_none_when_stalled` | Passing |
| AC-F09-02 | Given a receiver whose writes are slowed, then no more than `PIPELINE_DEPTH` chunks are buffered on either side | §29, §32 | T | planned: `session::tests::slow_writer_applies_backpressure` | Planned |
| AC-F09-03 | Given a 4 GiB sparse source file, then peak in-flight buffer memory stays under 40 MiB | §32 | T (ignored by default) | planned: `session::tests::memory_is_bounded_for_large_files` | Planned |
| AC-F09-04 | The engine contains no sleep, throttle or fixed rate | NFR-PERF-006 | I | Review checklist | Planned |
| AC-F09-05 | Stream counts never exceed 4 per transfer and 16 in total | FR-PAR-001 | T | planned: `session::tests::stream_caps_are_enforced` | Planned (D-F09-1 makes this trivial in v1) |
| AC-F09-06 | Over direct gigabit Ethernet, a 10 GB transfer sustains ≥ 100 MB/s | §31 | D | Manual | Planned — **feasible on the reference hardware** |
| AC-F09-07 | On a Thunderbolt pair, throughput meets the CR-12 bar | NFR-PERF-007 | D | Manual | **CONDITIONAL (§5.2)** |
| AC-F09-08 | Hashing cost is analysed against link speed before Phase 8 | §23, NFR-PERF-007 | A | Analysis note | Planned |

## 6. Out of scope

- Correctness of chunks and resume → F06.

## 7. Current implementation

- [`transfer.rs`](../src-tauri/src/transfer.rs): `PIPELINE_DEPTH`,
  `MAX_TOTAL_STREAMS`, `MAX_STREAMS_PER_TRANSFER`, `pipeline()`,
  `ThroughputEstimator`.
- [`transport.rs`](../src-tauri/src/transport.rs): 4 MiB socket buffers,
  `TCP_NODELAY` (uncommitted).
