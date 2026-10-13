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
