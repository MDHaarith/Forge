# Native PCB spike

This standalone crate changes nothing outside `spikes/pcb/`. It has its own
`[workspace]`; it is not a Forge workspace member. No app code, version, roadmap,
or plan checkbox was changed.

```sh
cd spikes/pcb
CARGO_TARGET_DIR=../../target cargo run --release
CARGO_TARGET_DIR=../../target cargo test
CARGO_TARGET_DIR=../../target cargo clippy --all-targets -- -D warnings
```

The program writes its printed measurements to `results.log` and regenerates the
artifacts listed below. Timings exclude compilation and the second routing run
used to assert determinism. Results vary with CPU load; the measured machine is
an Intel Core i5-1155G7, Linux x86_64. Five offline unit tests and the release
program's geometry/export assertions pass. Clippy passes with warnings denied.
Forge's workspace tests and TypeScript checks were not run: this crate is isolated
and this task expressly excludes app changes.

## Footprints: calculation versus installed KiCad

`footprint.rs` computes pads independently, then reads the installed `.kicad_mod`
files under `/usr/share/kicad/footprints`. Its small reader handles multiline
`pad` blocks, finds `at` and `size`, and excludes unnumbered paste-only pads.
It does not copy KiCad's pad dimensions into the calculator.

| Footprint | Calculated nominal pad, mm | Calculated centres, mm | Installed KiCad pad, mm | KiCad centres, mm | Largest difference, mm |
|---|---:|---:|---:|---:|---:|
| 0402 | 0.590 × 0.640 | 0.970 | 0.540 × 0.640 | 1.020 | 0.050 |
| 0603 | 0.875 × 0.950 | 1.575 | 0.800 × 0.950 | 1.650 | **0.075** |
| SOIC-8 | 1.950 × 0.600 | 4.950 | 1.950 × 0.600 | 4.950 | 0 |
| QFN-32 | 0.875 × 0.250 | 4.875 | 0.875 × 0.250 | 4.875 | 0 |

Centres means opposite pad rows, not adjacent-pin pitch. SOIC's pitch is 1.27 mm;
QFN's is 0.5 mm. The chosen QFN reference is
`QFN-32-1EP_5x5mm_P0.5mm_EP3.1x3.1mm`, with 32 perimeter pads and one 3.1 × 3.1 mm
exposed pad. The wildcard in the plan also matches different exposed-pad sizes;
freeze the exact filename before writing reference tests.

The inputs and goals are KiCad's published IPC-7351B generator definitions:
[RSS formulas](https://raw.githubusercontent.com/pointhi/kicad-footprint-generator/master/scripts/tools/ipc_pad_size_calculators.py),
[chip goals](https://raw.githubusercontent.com/pointhi/kicad-footprint-generator/master/scripts/SMD_chip_package_rlc-etc/ipc7351B_smd_two_terminal_chip.yaml),
[0402 dimensions](https://raw.githubusercontent.com/pointhi/kicad-footprint-generator/master/scripts/SMD_chip_package_rlc-etc/size_definitions/size_default_chip_devices_smaller_0603.yaml),
[0603 dimensions](https://raw.githubusercontent.com/pointhi/kicad-footprint-generator/master/scripts/SMD_chip_package_rlc-etc/size_definitions/size_default_chip_devices.yaml),
[IC goals](https://raw.githubusercontent.com/pointhi/kicad-footprint-generator/master/scripts/Packages/ipc_definitions.yaml),
[SOIC dimensions](https://raw.githubusercontent.com/pointhi/kicad-footprint-generator/master/scripts/Packages/Package_Gullwing__QFP_SOIC_SO/size_definitions/soic.yaml),
[QFN dimensions](https://raw.githubusercontent.com/pointhi/kicad-footprint-generator/master/scripts/Packages/Package_NoLead__DFN_QFN_LGA_SON/size_definitions/qfn/qfn-3x.yaml).
These are an older generator's dimensional inputs. The installed resistor
footprints instead cite IPC-SM-782 page 72. A package name and nominal density
alone do not fix manufacturing tolerances or termination lengths. This is a
calculation experiment, not a licensed IPC conformance assessment.

The calculation uses fabrication tolerance F=0.1 and placement tolerance P=0.05:

```text
Z = round(Lmin + 2*toe  + sqrt(CL² + F² + P²))
G = round(Smax - 2*heel - sqrt(CS² + F² + P²))
X = round(Wmin + 2*side + sqrt(CW² + F² + P²))
pad length = (Z-G)/2; centre spacing = (Z+G)/2
```

For IC terminals, independent lead tolerances give
`CS = sqrt(CL² + 2*CT²)`. Reduce the worst-case S interval symmetrically to its
RSS interval before using Smax; using `Smin + CS` gives the wrong pad length.
The chip calculations use the published direct termination-spacing interval.

## Router

Four orthogonal neighbours per layer plus a layer change implement A*. The grid
is `max(clearance/2, 0.05)`; here it is 0.1 mm. A step costs 10 and a new via 300,
equivalent to a 3 mm routing penalty. Existing same-net through-hole pads change
layers without another via. Pads, holes, keepouts, other-net tracks and vias
block the search. Via clearance is checked on both layers with the full 0.6 mm
copper diameter. Nets start short-first. Failed nets move ahead within the
short-first groups during up to three full rip-up passes. The best pass is kept.

Tracks attach to exact pad centres through their snapped grid points. Straight
sections are coalesced. An independent continuous-geometry check measures
track/track, track/pad, via/pad, via/track, via/via and hole gaps. Routes and vias
must equal a second run exactly; timing text is not compared.

| Board | Parts / pads | Nets routed / unrouted | Links | Vias | Passes | Release time |
|---|---:|---:|---:|---:|---:|---:|
| LED example | 4 / 14 | 4 / 0 | 4 | 1 | 1 | about 9 ms |
| Generated 50 × 40 mm, two layers | 30 / 430 | 26 / 14 | 26 | 40 | 3 | about 3.4 s |

The LED board uses section 3.1's dimensions, rotations, nets, hole and keepout.
U1, whose `at` is omitted, gets a free fixed position `[0,4]` on the bottom.
Its labelled pins merge into VCC and GND. Bottom footprints mirror their local
x coordinates before rotation. Header pins are centred vertically
about `[0,-6]`. GND uses 0.4 mm traces; other routed nets use 0.2 mm.
The four routed net count includes singleton VCC: three nets actually need
routing, and GND needs two links. Minimum foreign-copper gap is 0.225 mm.

The generated board has ten SOIC-8, ten QFN-32 and ten 0603 packages in a 6 × 5
placement grid. Every fifth part is bottom-mounted. Fixed-seed Fisher-Yates
(`0x7351_b032`) picks 80 unique terminals for 40 two-pin nets. Unassigned copper
pads remain obstacles. Minimum routed gap is 0.200 mm. The 14 failures remain
visible as red pads in `generated-router.svg`; they are not hidden or credited
as routed. This dense test disproves any assumption that fewer than 60 parts
guarantees routing success. It does not prove those nets physically impossible.

## Pour decision: use i_overlay

Cadrum 0.9 exposes face inspection but no public face constructor or face
boolean operation. The attempted zero-thickness extrusion returns
`Extrude failed: direction is zero-length`. Therefore a genuine planar Cadrum
benchmark cannot be built through its public API.

The nearest available comparison builds the identical polygons as 0.035 mm
solids and performs a single Cadrum boolean expression. Twenty-two operands take
about **1.30 seconds**, exceeding the plan's one-second limit. The result has
537.770513 mm² of top-face area; volume divided by thickness agrees.

`i_overlay` 4.5.2 performs planar difference/union/intersection directly in about
**0.97 ms**, yielding 537.770512 mm². Area differs from Cadrum by 0.000000399 mm²;
all 610 point comparisons agree, including ten targeted thermal checks.
The bottom GND pour has two thermal pads, eight 0.3 mm spokes, 0.2 mm clearance,
one component and 19 inner contours. Same-net vias join solidly. Foreign copper
is subtracted again after adding spokes so reliefs cannot create shorts. Drills
are also cut. Islands under 1 mm² are filtered; this LED result has none to remove.

Clearance arcs use 64-segment circumscribed approximations; they expand, rather
than shrink, the requested gap. The result preserves hole contours. Choose
`i_overlay` for native planar booleans and directly usable Gerber/SVG contours.
Keep Cadrum for solids and 3D export. The spike pins the resolved versions in
Cargo.lock; upgrading the polygon library needs the same area/thermal checks.

Dependencies: `cadrum` reuses Forge's kernel and GLB writer; `i_overlay` removes
artificial-thickness booleans and contour recovery; `serde_json`, already used
by Forge, parses GLB metadata for verification. No Gerber or glTF dependency was
added, and no installed EDA/CAD program runs.

## Fab and 3D artifacts

- `led-F_Cu.gbr`, `led-B_Cu.gbr`, `led-Edge_Cuts.gbr`: Gerber X2, mm, absolute
  4.6 coordinates, file-function and polarity attributes, aperture definitions,
  net/component attributes, flashes, linear draws and closed regions.
- `led.drl`: explicit-decimal metric Excellon, three tools and four holes:
  two 1 mm header drills, one 0.3 mm via and one 3.2 mm mounting hole.
- `gerber-preview.svg`: own minimal reader reads the front copper, edge and drill
  files into one SVG. Its second panel reads bottom copper, including the pour.
  Seven front flashes, sixteen front draws and four edge segments round-trip.
  Serialized bottom region area differs by 0.000009927 mm² after 1 µm-coordinate
  quantization. Bottom uses top-view coordinates, so it is not mirrored.
- `generated-router.svg`: both layers of the dense routing experiment.
- `led.step`: board slab with all drills plus four package boxes, five solids.
  Total volume 1088.593365 mm³; STEP import differs by about 4e-12 mm³. Slab
  construction takes about 10–13 ms; STEP writing/import take tens of milliseconds.
- `led.glb`: Cadrum mesh and `Mesh::write_gltf_binary`; 76,000 bytes, 2,144 vertices,
  2,092 triangles, four primitives. Meshing takes about 30 ms; writing about
  0.6 ms. Header/chunk lengths, JSON, buffer bounds, triangle counts, indices and
  binary positions are checked. Forge-space bounds are 30 × 20 × 11.85 mm.
- `assembly.svg` and `preview.html`: kernel mesh preview and self-contained
  artifact page. The HTML was rendered and visually checked.

`grep -rn glb server/src` finds consumers and external-engine output descriptions,
not a Rust GLB writer. **Cadrum itself already has one.** It writes mesh coordinates
unchanged. Forge's existing viewer rotates glTF +90° around X and uses mm;
transform `[x,y,z]` into `[x,z,-y]` before writing, including normals and edge
polylines. No metre scaling is applied, matching the current Forge pipeline.
External glTF tools may interpret those numeric units as metres.

The preview reader intentionally supports only the subset emitted here. It
rejects unsupported geometry such as arcs. SMT Gerber pads are rectangular
rather than KiCad's rounded rectangles. The drill preview combines plated and
non-plated holes; production export must split PTH and NPTH, as the plan already
requires. There is no general spec parser, auto-placement engine, schematic,
full DRC, four-layer experiment, independent CAM validation, or app integration.

## Plan corrections and arithmetic

1. The 0603 compatibility promise is not satisfied by these published nominal
   inputs. `Z = round(1.5 + 0.7 + sqrt(0.2² + 0.1² + 0.05²), 0.05) = 2.45`.
   `G = round(1.11 - sqrt(0.41² + 0.1² + 0.05²), 0.05) = 0.70`.
   Thus pad length is `(2.45-0.70)/2 = 0.875` and centres are
   `(2.45+0.70)/2 = 1.575`. Against installed KiCad, both differences are
   `0.075 > 0.05`, exceeding the proposed tolerance by `0.025` mm. Freeze the
   modern IPC-SM-782 dimensional inputs before fixing tests; do not fit fillet
   goals to make this comparison pass. 0402 is exactly at the 0.05 mm boundary.
2. Section 3.1's keepout lies wholly outside the board:
   `y = 16 ± 8/2 = [12,20]`, while the board has `y = ±20/2 = [-10,10]`.
   Since `12 > 10`, intersection is zero. This example does not exercise an
   on-board keepout; the router unit test uses one that blocks both layers.
3. Merging labelled pins makes GND have `2 + 1 = 3` terminals, needing
   `3-1 = 2` links. VCC has one terminal and zero links. IN and A each need one;
   total links `1 + 1 + 2 + 0 = 4`. The example leaves U1.VCC unpowered: J1.1
   belongs to IN, not VCC. It is a geometry test, not a working blinker circuit.
4. The nominal grid is correctly `0.2/2 = 0.1` mm. It creates
   `(30/0.1+1)*(20/0.1+1)*2 = 121,002` LED states and
   `(50/0.1+1)*(40/0.1+1)*2 = 401,802` generated-board states. The generated board
   has `10*8 + 10*33 + 10*2 = 430` pads. With `30 < 60` parts it still leaves
   14 nets unrouted. Track congestion, pad pitch and obstacle count belong in
   the router's upgrade criterion, not just part count.
