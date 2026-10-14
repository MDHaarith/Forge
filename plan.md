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
- [ ] Batch 5: panelization (rails, tabs, V-score, fiducials), IPC-D-356 netlist, stencil paste
  reduction, fab drawing (stackup + drill table), true slots in the 3D export.
- [ ] Batch 6: interactive editing in the UI (move parts, lock tracks, router keeps locked copper),
  KiCad `.kicad_mod` footprint import.
- [ ] Batch 7: native circuit simulator (DC operating point, transient; R, L, C, diode, BJT, MOSFET).
