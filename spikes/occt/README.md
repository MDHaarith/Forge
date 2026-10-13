# OCCT binding spike

Standalone crate; its empty `[workspace]` keeps it outside Forge's workspace.
All experiment code, caches, tools, output files, and logs stay in this directory.

## Binding selection

1. `opencascade = "0.3"` resolves to 0.3.0. Rejected after inspecting its
   downloaded source, before compiling: several required operations are absent
   from the public API. See `attempts/opencascade.md` for exact gaps and raw-binding
   extension options. No runtime or build measurements are claimed for it.
2. `cadrum = "0.9"` resolves to 0.9.0. All requested runtime checks pass.
   It downloads prebuilt, static OCCT 8.0.1 libraries for x86_64 Linux and compiles
   its C++/cxx bridge locally. No installed OCCT or CAD application is required.
3. `occt-wasm = "5"` was not tried because cadrum passes.

`cadrum` exposes cylinder surface data, face projection with an outward unit
normal, exact volume, solid point classification, STEP read/write, and meshing.
Its mesh writer emits binary STL; the experiment validates the header's triangle
count and exact file length.

There is no public UV-bounds or UV-evaluation API. For this complete cylindrical
hole, its UV midpoint is reconstructed from the vertical seam: the point opposite
the seam at half its height. `Face::project` evaluates the outward normal there.
This does not generalize to arbitrarily trimmed faces. There is no public raw
OCCT access or separate cadrum `-sys` crate: `occt::ffi` and shape handles are
private. Adding general UV operations requires a fork extending its existing
`src/ffi.rs`, `src/ffi.h`, `src/ffi.cpp`, and face API, or an independent bridge
that loads shapes itself. No unsafe layout tricks or local crate patches were used.

The tessellation uses absolute linear deflection 0.1 mm and angular deflection
0.1 radians. Triangle winding defines outward normals. Only triangles below
the requested normal-Z threshold contribute, and each contributing triangle
is asserted to lie on the bottom face.

## Reproduce

```sh
cd spikes/occt
export CARGO_HOME="$PWD/.cargo-home"
export TMPDIR="$PWD/tmp"
mkdir -p "$TMPDIR"
cargo run --release
cargo clean
/usr/bin/time -p cargo build --release
stat --format='%s' target/release/forge-occt-spike
```

`results.log` records the assertion-based checks; `clean-build.log` and
`clean-build-time.txt` record the final clean build. Timing includes downloading
the prebuilt OCCT archive after `cargo clean` removes its cache inside `target`.
Cargo registry downloads are already cached. Binary size is unstripped decimal
MB (bytes / 1,000,000).

Measured clean release build: 37.84 seconds (0.630667 minutes).
Unstripped release binary: 34,020,152 bytes (34.020152 MB).
The final build emitted no warnings. The rebuilt binary passed every assertion.
`ldd` reports only standard C/C++ system runtimes, with no dynamic OCCT libraries.

Forge workspace tests and UI typechecks were skipped because no Forge app code
or workspace dependency changed. No commit, version bump, plan update, or edits
outside this directory were made.

## Stage 2

Run `CARGO_TARGET_DIR=../../target cargo run --release --bin stage2` from this
directory. `results-stage2.log` contains the full successful run, including
assertions and timings. `stage2.svg` contains front, top, right and iso views;
its layout and dashed hidden lines were checked in the headless browser.

| Check | Measured result |
|---|---|
| External M6×1, length 10 | 229.570530 mm³ (+2.1040%), one solid |
| External bounds after meshing | [[−3.005243,−3.005244,−0.005238],[3.005247,3.005248,10.005238]]; within 0.01 |
| Internal M6, through 20×20×10 | Removed 231.332891 mm³ (+2.8878%), one remaining solid |
| L sheet, thickness 2, width 10, flanges 30/20, radius 2 | 1094.247780 mm³; bounds [[0,0,0],[34,10,24]]; flat [54.523893,10] |
| Line/arc/line sweep, circle r2 | 448.719503773 mm³ |
| Six polar cylinders r2 h10 at radius 10 | 753.982237 mm³, six solids |
| Cube clearance at [0,0,0] and [13,14,0] | 5.000000 mm from 0.01 mm meshes |

The alleged thread placement error is **an unmeshed bounding-box error**.
The 10-turn sweep reports radius 6.779455 before meshing, but its vertices
reach only 3.000070. Neither profile placement nor `Up(Z)` needs changing.
cadrum's `shape_bounding_box` calls `BRepBndLib::Add`, which uses conservative
surface bounds until triangulation exists. [OCCT documents this behavior](https://occt3d.com/dev/doc/refman/html/class_b_rep_bnd_lib.html).
The public-API workaround is to mesh **the final solid** before querying its
box; 0.005 mm absolute deflection passes the requested 0.01 mm bound check.
Do not round or clamp the oversized box to disguise it.

The original trapezoid also had incorrect ISO flank widths. The basic profile
uses `H = sqrt(3)*P/2`, minor radius `d/2 − 5H/8`, crest width `P/8` and
root width `P/4`. Keep 60° flanks when extending the base 0.01 mm into the
core. Extend **both** core and helix by one pitch at each end, then fuse and
clip. A 10 mm core inside the extended 12 mm helix produced a ridge-only
union during this experiment. This exact working public-API pattern avoids it:

```rust
let minor = d / 2.0 - 5.0 * (3.0_f64.sqrt() * pitch / 2.0) / 8.0;
let base = minor - 0.01;
let half_base = 3.0 * pitch / 8.0 + 0.01 / 3.0_f64.sqrt();
let helix = Edge::helix(base, pitch, length + 2.0 * pitch, DVec3::Z, DVec3::X)?
    .translate(-DVec3::Z * pitch);
let profile = Edge::polygon(&[
    DVec3::new(base, 0.0, -pitch - half_base),
    DVec3::new(d / 2.0, 0.0, -pitch - pitch / 16.0),
    DVec3::new(d / 2.0, 0.0, -pitch + pitch / 16.0),
    DVec3::new(base, 0.0, -pitch + half_base),
])?;
let ridge = Solid::sweep(profile.iter(), [&helix], ProfileOrient::Up(DVec3::Z))?;
let core = Solid::cylinder(minor, DVec3::Z * (length + 2.0 * pitch))
    .translate(-DVec3::Z * pitch);
let fused = (cadrum::Boolean::<Solid>::default() + &core + &ridge).build()?;
let rod = (cadrum::Boolean::<Solid>::from(&fused) * &clip).build()?;
let mesh = Solid::mesh([&rod], Tessellation {
    deflection_linear: 0.005,
    deflection_angular: 0.1,
    relative_linear: false,
})?;
let bbox = rod.bounding_box();
```

Here `clip` is a box wider than the rod, with Z bounds `[0,length]`.
For the internal thread, subtract both the diameter-5 tap drill and the
translated rod: `(Boolean::from(&box20) - &tap - &cutter).build_vec()`.
The external run took 5643 ms including placement diagnostics; the extended
ridge sweep took 10 ms, fuse/clip 1060 ms, final meshing 1528 ms, and the
internal cut 1077 ms. Exact basic-profile integration gives 229.572040 mm³;
the plan's 224.84 is a pitch-diameter approximation, within its stated 3%.

Sheet metal needs no workaround: extrude one closed XZ section containing
inner and outer circular arcs along +Y. Composite sweep uses
`circle.align_z(start_tangent, DVec3::Y).translate(start)`, then
`Solid::sweep([&profile], path.iter(), ProfileOrient::Up(DVec3::Z))`.
For SVG, split scene arrays at NaNs, remove duplicate fragments and join
matching endpoints (`joined_polylines` in `stage2.rs`) before writing paths.
Otherwise every short fragment restarts `stroke-dasharray="2 1"`, making
hidden circles look solid. Negate scene Y when writing SVG coordinates.

**Plan correction:** §3.1's composite sweep reference must round to **448.720**,
not 448.719: `π·2²·(10 + π·10/2 + 10) = 448.719500309`. Using the plan's
rounded arc-through point gives 448.719503773, also rounding to 448.720.
The sheet reference is correct: `(2·(30+20) + π/4·(4²−2²))·10 = 1094.247780`.

The clearance check proves the requested cubes only. Vertex-to-triangle in
both directions omits edge-to-edge minima; §3.2's claim that this is always
exact for flat faces is too broad. Add edge-pair distances for general meshes.
The spike and `cargo clippy --release --bin stage2 -- -D warnings` pass.
Forge workspace/UI checks were skipped; no app code or dependency changed.
