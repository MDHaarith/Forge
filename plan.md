# Plan: PCB engine to full CAD

Earlier plans are in git: stage 3 (native PCB engine, done in v0.17.0) at `094e783a:plan.md`.
Stage 3 listed interactive editing and foreign importers as skipped; the owner asked for them on
2026-10-14, so batch 6 brings them back.

Each batch: Codex implements from an exact brief, Gemini writes black-box tests
(`server/tests/pcb_batchN.rs`), Claude reviews, runs `cargo test --workspace` + `npm run typecheck`,
bumps the version and commits.

- [x] Batch 2 (v0.20.0): wired schematic, ERC warnings, rounded SMD pads, 11 KiCad footprints.
- [x] Batch 3 (v0.21.0): router for 200-part boards. New 200-part / ~400-net 4-layer benchmark with inner
  GND/power planes; 100 % routed, 0 DRC errors, deterministic, < ~10 min release. Raise the 8M-cell
  grid limit (compact cells), plane vias for power pins, regional routing, multi-core, DRC spatial index.
- [x] Batch 4 (v0.22.0): differential pairs, length matching (meanders), impedance-driven widths from a stackup,
  blind/buried vias, teardrops.
- [x] Batch 4b (v0.23.0): router beautification (octilinear pulling, chamfers, via reduction and clean pad entries).
- [x] Batch 5 (v0.24.0): panelization (rails, tabs, V-score, fiducials), IPC-D-356 netlist, stencil paste
  reduction, fab drawing (stackup + drill table), true slots in the 3D export.
- [x] Batch 6 (v0.25.0): interactive editing in the UI (move parts, lock tracks, router keeps locked copper),
  KiCad `.kicad_mod` footprint import.
- [x] Batch 7 (v0.26.0): native circuit simulator (DC operating point/sweeps, adaptive transient;
  R, L, C, diode, BJT, MOSFET, E/G sources and nested subcircuits), analytic accuracy fixtures and deterministic plots.
- [x] Batch 8 (v0.27.0): AC small-signal and noise analyses, temperature/model scaling and resistor
  TC1/TC2, Bode/noise plots, and offline ngspice accuracy fixtures with an optional live replay.
- [x] Batch 9 (v0.28.0): retire digital logic and the redundant scripted CAD renderer; native gitoxide
  project creation, status, staging, commits and logs, with offline identity/ignore/path regression tests.
