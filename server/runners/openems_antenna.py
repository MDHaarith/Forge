"""Simulates a PCB antenna from pcb/<name>.circuit.json with openEMS (FDTD).

    python openems_antenna.py '<json args>'

Args: circuit (path), out (directory), antenna (component name or ""),
f0_hz, span_hz, quality ("fast" | "accurate"), er, tand, threads, sim_dir.

The model is the antenna's own copper, the board substrate, every copper pour and every via.
Other parts and their traces are left out, so the result is the antenna as the matching
network sees it. Units are millimetres. Prints one line "FORGE_RESULT <json>" at the end,
or "FORGE_ERROR <message>" for a problem with the board.
"""

import json
import math
import os
import re
import sys

import numpy as np

C0 = 299_792_458.0
EPS0 = 8.8541878128e-12


class BoardError(Exception):
    pass


# ---------- geometry from circuit.json ----------

def rect(cx, cy, w, h, rot_deg=0.0):
    pts = [(-w / 2, -h / 2), (w / 2, -h / 2), (w / 2, h / 2), (-w / 2, h / 2)]
    a = math.radians(rot_deg)
    c, s = math.cos(a), math.sin(a)
    return [(cx + x * c - y * s, cy + x * s + y * c) for x, y in pts]


def ngon(cx, cy, r, n=8):
    # Rotated by half a step so the polygon has edges on the axes, like a pad.
    return [(cx + r * math.cos(2 * math.pi * (i + 0.5) / n), cy + r * math.sin(2 * math.pi * (i + 0.5) / n)) for i in range(n)]


def segment(a, b, w):
    (x1, y1), (x2, y2) = a, b
    length = math.hypot(x2 - x1, y2 - y1)
    if length < 1e-9:
        return None
    ux, uy = (x2 - x1) / length, (y2 - y1) / length
    nx, ny = -uy * w / 2, ux * w / 2
    return [(x1 + nx, y1 + ny), (x2 + nx, y2 + ny), (x2 - nx, y2 - ny), (x1 - nx, y1 - ny)]


def axis_aligned(a, b):
    return abs(a[0] - b[0]) < 1e-9 or abs(a[1] - b[1]) < 1e-9


def pad_shape(e):
    shape = e.get("shape", "rect")
    x, y = e.get("x", 0.0), e.get("y", 0.0)
    if shape in ("rect", "rotated_rect", "pill", "rotated_pill"):
        return rect(x, y, e["width"], e["height"], e.get("ccw_rotation", e.get("rotation", 0.0)) or 0.0)
    if shape == "circle":
        return ngon(x, y, e["radius"], 16)
    if shape == "polygon":
        return [(p["x"], p["y"]) for p in e["points"]]
    raise BoardError(f"unsupported pad shape {shape}")


def ring(vertices):
    return [(v["x"], v["y"]) for v in vertices]


def pour_shapes(e):
    """(outer polygon, [hole polygons]) for a pcb_copper_pour."""
    shape = e.get("shape")
    if shape == "brep":
        b = e["brep_shape"]
        return ring(b["outer_ring"]["vertices"]), [ring(r["vertices"]) for r in b.get("inner_rings", [])]
    if shape == "rect":
        c = e["center"]
        return rect(c["x"], c["y"], e["width"], e["height"], e.get("rotation", 0.0) or 0.0), []
    if shape == "polygon":
        return [(p["x"], p["y"]) for p in e["points"]], []
    raise BoardError(f"unsupported copper pour shape {shape}")


def inside(poly, x, y):
    hit = False
    n = len(poly)
    for i in range(n):
        (x1, y1), (x2, y2) = poly[i], poly[(i + 1) % n]
        if (y1 > y) != (y2 > y) and x < x1 + (y - y1) * (x2 - x1) / (y2 - y1):
            hit = not hit
    return hit


def layer_z(board):
    t = float(board.get("thickness", 1.6))
    layers = int(board.get("num_layers", 2))
    z = {"bottom": 0.0, "top": t}
    if layers >= 4:
        # JLCPCB 7628 stackup: 0.21 mm prepreg between each outer layer and its neighbour.
        d = 0.2104 if t >= 1.0 else t / 8
        z["inner1"], z["inner2"] = t - d, d
    return z


def load_board(circuit, antenna_name):
    by_type = {}
    for e in circuit:
        by_type.setdefault(e["type"], []).append(e)
    boards = by_type.get("pcb_board", [])
    if not boards:
        raise BoardError("the circuit has no board")
    board = boards[0]
    z = layer_z(board)

    names = {s["source_component_id"]: s.get("name", "") for s in by_type.get("source_component", [])}
    comps = {c["pcb_component_id"]: names.get(c.get("source_component_id"), "") for c in by_type.get("pcb_component", [])}
    # Antenna copper traces belong to their component.
    drawn = sorted({t["pcb_component_id"] for t in by_type.get("pcb_trace", []) if t.get("pcb_component_id")})
    if antenna_name:
        ids = [cid for cid, n in comps.items() if n == antenna_name]
        if not ids:
            raise BoardError(f"no component named {antenna_name}")
        ant = ids[0]
    elif len(drawn) == 1:
        ant = drawn[0]
    elif not drawn:
        raise BoardError("no antenna found; add an <antenna /> to the board")
    else:
        raise BoardError("several antennas on the board; name one: " + ", ".join(comps[c] for c in drawn))

    copper = []  # (layer, polygon, is_antenna)
    arm = []  # points along the antenna's traces, for the ground-clearance check
    for t in by_type.get("pcb_trace", []):
        if t.get("pcb_component_id") != ant:
            continue
        wires = [p for p in t["route"] if p.get("route_type") == "wire"]
        for a, b in zip(wires, wires[1:]):
            n = max(1, int(math.hypot(b["x"] - a["x"], b["y"] - a["y"]) / 0.25))
            arm.extend((a["x"] + (b["x"] - a["x"]) * k / n, a["y"] + (b["y"] - a["y"]) * k / n) for k in range(n + 1))
            if a.get("layer") != b.get("layer"):
                continue
            pa, pb = (a["x"], a["y"]), (b["x"], b["y"])
            poly = segment(pa, pb, a["width"])
            if poly:
                copper.append((a["layer"], poly))
        for i, p in enumerate(wires):
            w = p["width"]
            nbrs = [wires[j] for j in (i - 1, i + 1) if 0 <= j < len(wires)]
            square = all(axis_aligned((p["x"], p["y"]), (q["x"], q["y"])) for q in nbrs)
            copper.append((p["layer"], rect(p["x"], p["y"], w, w) if square else ngon(p["x"], p["y"], w / 2)))

    pads = {}
    for p in by_type.get("pcb_smtpad", []):
        if p.get("pcb_component_id") == ant:
            copper.append((p["layer"], pad_shape(p)))
            for hint in p.get("port_hints", []):
                pads[hint] = ("smt", p)

    barrels = []  # (x, y, hole radius)
    for h in by_type.get("pcb_plated_hole", []) + by_type.get("pcb_via", []):
        if h["type"] == "pcb_plated_hole" and h.get("pcb_component_id") != ant:
            continue
        x, y = h["x"], h["y"]
        outer = h.get("outer_diameter", h.get("outer_width", 0.6))
        hole = h.get("hole_diameter", h.get("hole_width", 0.3))
        names_ = h.get("layers") or [h.get("from_layer", "top"), h.get("to_layer", "bottom")]
        for layer in names_:
            if layer in z:
                copper.append((layer, ngon(x, y, outer / 2, 16)))
        barrels.append((x, y, hole / 2, [z[l] for l in names_ if l in z]))
        if h["type"] == "pcb_plated_hole":
            for hint in h.get("port_hints", []):
                pads[hint] = ("hole", h)

    pours = []  # (layer, outer, holes)
    for e in by_type.get("pcb_copper_pour", []):
        if e.get("layer") in z:
            outer, holes = pour_shapes(e)
            pours.append((e["layer"], outer, holes))

    if "pin1" not in pads:
        raise BoardError(f"antenna {comps[ant]} has no pin1 feed pad")
    return board, z, comps[ant], copper, barrels, pours, pads, arm


def covered(pours, x, y):
    return any(inside(outer, x, y) and not any(inside(h, x, y) for h in holes) for _, outer, holes in pours)


def ground_under(arm, pads, pours, clear=3.0, near=2.0):
    """Fraction of the antenna's trace length with ground pour under it or within `near` mm
    beside it, leaving out the first few millimetres from the feed and the ground short,
    where ground belongs."""
    ends = [(p["x"], p["y"]) for kind, p in pads.values()]
    far = [(x, y) for x, y in arm if all(math.hypot(x - ex, y - ey) > clear for ex, ey in ends)]
    if not far:
        return 0.0
    ring_ = [(near * math.cos(k * math.pi / 4), near * math.sin(k * math.pi / 4)) for k in range(8)]

    def grounded(x, y):
        return covered(pours, x, y) or any(covered(pours, x + dx, y + dy) for dx, dy in ring_)

    return sum(1 for x, y in far if grounded(x, y)) / len(far)


def feed_port(z, pads, pours):
    """Where the lumped port goes: (start, stop, direction, description)."""
    kind, p1 = pads["pin1"]
    x, y = p1["x"], p1["y"]
    pad_layer = p1["layer"] if kind == "smt" else "top"
    zp = z[pad_layer]
    second = pads.get("pin2")
    if second and second[0] == "smt" and second[1]["layer"] == pad_layer:
        # Balanced feed (folded dipole): across the gap between the two pads.
        p2 = second[1]
        dx, dy = p2["x"] - x, p2["y"] - y
        if abs(dy) < 1e-6:
            a = min(x, p2["x"]) + (p1["width"] if x < p2["x"] else p2["width"]) / 2
            b = max(x, p2["x"]) - (p2["width"] if x < p2["x"] else p1["width"]) / 2
            sign = 1 if dx > 0 else -1
            return [a, y, zp], [b, y, zp], "x", sign, "across the gap between pin1 and pin2"
        if abs(dx) < 1e-6:
            a = min(y, p2["y"]) + (p1["height"] if y < p2["y"] else p2["height"]) / 2
            b = max(y, p2["y"]) - (p2["height"] if y < p2["y"] else p1["height"]) / 2
            sign = 1 if dy > 0 else -1
            return [x, a, zp], [x, b, zp], "y", sign, "across the gap between pin1 and pin2"
        raise BoardError("pin1 and pin2 of the antenna must line up along x or y")
    # Unbalanced feed: from the nearest ground copper under the pad, through the board.
    below = [
        z[layer] for layer, outer, holes in pours
        if layer != pad_layer and inside(outer, x, y) and not any(inside(h, x, y) for h in holes)
    ]
    if not below:
        raise BoardError(
            f"no ground copper under the antenna feed at ({x:g}, {y:g}) mm; add <copperpour connectsTo=\"net.GND\" layer=\"bottom\" /> "
            "so the feed has a ground reference"
        )
    zg = min(below, key=lambda v: abs(v - zp))
    lo, hi = sorted((zg, zp))
    return [x, y, lo], [x, y, hi], "z", 1 if zp > zg else -1, f"from the ground plane up to the pin1 pad ({abs(zp - zg):g} mm probe)"


# ---------- mesh ----------

def edges(poly):
    """x positions of vertical edges and y positions of horizontal edges."""
    xs, ys = [], []
    for (x1, y1), (x2, y2) in zip(poly, poly[1:] + poly[:1]):
        if abs(x1 - x2) < 1e-9:
            xs.append(x1)
        if abs(y1 - y2) < 1e-9:
            ys.append(y1)
    return xs, ys


def thirds(poly, is_metal, r):
    """Mesh lines for a polygon's axis-aligned edges by the thirds rule: one line r/3 inside the
    copper and one 2r/3 outside, which puts the edge's field singularity where FDTD expects it.
    Edges with copper on both sides (inside a merged shape) get none."""
    xs, ys = [], []
    eps = 1e-4
    for (x1, y1), (x2, y2) in zip(poly, poly[1:] + poly[:1]):
        if abs(x1 - x2) < 1e-9 and abs(y1 - y2) > 1e-9:
            mid = (y1 + y2) / 2
            lo, hi = is_metal(x1 - eps, mid), is_metal(x1 + eps, mid)
            if lo != hi:
                s = 1 if hi else -1
                xs += [x1 + s * r / 3, x1 - s * 2 * r / 3]
        if abs(y1 - y2) < 1e-9 and abs(x1 - x2) > 1e-9:
            mid = (x1 + x2) / 2
            lo, hi = is_metal(mid, y1 - eps), is_metal(mid, y1 + eps)
            if lo != hi:
                s = 1 if hi else -1
                ys += [y1 + s * r / 3, y1 - s * 2 * r / 3]
    return xs, ys


def merge(lines, fixed, tol):
    """Sorted lines with neighbours closer than tol merged; lines in `fixed` always survive."""
    out = []
    for v in sorted(set(round(v, 6) for v in list(lines) + list(fixed))):
        if out and v - out[-1] < tol and not (v in fixed and out[-1] in fixed):
            if v in fixed:
                out[-1] = v
            continue
        out.append(v)
    return out


# ---------- plots ----------

SVG_STYLE = 'font-family="sans-serif" font-size="12"'


def s11_svg(f, s11_db, f0, title):
    w, h, l, r, t, b = 640, 360, 56, 16, 30, 40
    lo = min(-30.0, math.floor(min(s11_db) / 5) * 5)
    fx = lambda v: l + (v - f[0]) / (f[-1] - f[0]) * (w - l - r)
    fy = lambda v: t + (0 - v) / (0 - lo) * (h - t - b)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" {SVG_STYLE}>', f'<rect width="{w}" height="{h}" fill="#fff"/>']
    for d in range(0, int(lo) - 1, -5):
        out.append(f'<line x1="{l}" x2="{w - r}" y1="{fy(d):.1f}" y2="{fy(d):.1f}" stroke="#ddd"/><text x="{l - 6}" y="{fy(d) + 4:.1f}" text-anchor="end">{d}</text>')
    step = 10 ** math.floor(math.log10((f[-1] - f[0]) / 1e6 / 4)) * 1e6
    for k in np.arange(math.ceil(f[0] / step) * step, f[-1] + 1, step):
        out.append(f'<line x1="{fx(k):.1f}" x2="{fx(k):.1f}" y1="{t}" y2="{h - b}" stroke="#eee"/><text x="{fx(k):.1f}" y="{h - b + 16}" text-anchor="middle">{k / 1e6:g}</text>')
    out.append(f'<line x1="{l}" x2="{w - r}" y1="{fy(-10):.1f}" y2="{fy(-10):.1f}" stroke="#c33" stroke-dasharray="5 4"/>')
    out.append(f'<line x1="{fx(f0):.1f}" x2="{fx(f0):.1f}" y1="{t}" y2="{h - b}" stroke="#36c" stroke-dasharray="3 3"/>')
    pts = " ".join(f"{fx(a):.1f},{fy(max(lo, v)):.1f}" for a, v in zip(f, s11_db))
    out.append(f'<polyline points="{pts}" fill="none" stroke="#111" stroke-width="2"/>')
    out.append(f'<text x="{l}" y="18" font-weight="bold">{title}</text>')
    out.append(f'<text x="{(w + l - r) / 2}" y="{h - 8}" text-anchor="middle">Frequency (MHz)</text>')
    out.append(f'<text x="14" y="{(h + t - b) / 2}" transform="rotate(-90 14 {(h + t - b) / 2})" text-anchor="middle">S11 (dB)</text>')
    out.append("</svg>")
    return "\n".join(out)


def pattern_svg(cuts, peak, title):
    """cuts: [(label, colour, angles in degrees, gain dBi)]; polar, outer ring at the peak."""
    size, cx, cy, R = 420, 210, 220, 160
    top = math.ceil(peak / 5) * 5
    floor = top - 30
    rad = lambda g: R * (max(g, floor) - floor) / (top - floor)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size + 140} {size + 20}" {SVG_STYLE}>', f'<rect width="{size + 140}" height="{size + 20}" fill="#fff"/>']
    for g in range(floor, top + 1, 10):
        out.append(f'<circle cx="{cx}" cy="{cy}" r="{rad(g):.1f}" fill="none" stroke="#ddd"/><text x="{cx + 3}" y="{cy - rad(g) + 12:.1f}" fill="#888">{g} dBi</text>')
    for a in range(0, 360, 30):
        x, y = cx + R * math.sin(math.radians(a)), cy - R * math.cos(math.radians(a))
        out.append(f'<line x1="{cx}" y1="{cy}" x2="{x:.1f}" y2="{y:.1f}" stroke="#eee"/>')
        out.append(f'<text x="{cx + (R + 14) * math.sin(math.radians(a)):.1f}" y="{cy - (R + 14) * math.cos(math.radians(a)) + 4:.1f}" text-anchor="middle" fill="#888">{a}°</text>')
    for i, (label, colour, angles, gain) in enumerate(cuts):
        pts = " ".join(f"{cx + rad(g) * math.sin(math.radians(a)):.1f},{cy - rad(g) * math.cos(math.radians(a)):.1f}" for a, g in zip(angles, gain))
        out.append(f'<polygon points="{pts}" fill="none" stroke="{colour}" stroke-width="2"/>')
        out.append(f'<line x1="{size}" x2="{size + 20}" y1="{60 + 20 * i}" y2="{60 + 20 * i}" stroke="{colour}" stroke-width="2"/><text x="{size + 26}" y="{64 + 20 * i}">{label}</text>')
    out.append(f'<text x="10" y="18" font-weight="bold">{title}</text>')
    out.append("</svg>")
    return "\n".join(out)


# ---------- simulation ----------

def simulate(a):
    from CSXCAD import ContinuousStructure
    from openEMS import openEMS

    with open(a["circuit"]) as fh:
        circuit = json.load(fh)
    board, z, ant_name, copper, barrels, pours, pads, arm = load_board(circuit, a.get("antenna") or "")
    grounded = ground_under(arm, pads, pours)
    (start, stop, port_dir, sign, port_note) = feed_port(z, pads, pours)

    f0, span = float(a["f0_hz"]), float(a["span_hz"])
    fmin, fmax = f0 - span / 2, f0 + span / 2
    accurate = a.get("quality") == "accurate"
    er, tand = float(a["er"]), float(a["tand"])
    t = float(board.get("thickness", 1.6))

    fdtd = openEMS(NrTS=200_000 if accurate else 80_000, EndCriteria=1e-4 if accurate else 10 ** (-3))
    fdtd.SetGaussExcite(f0, span / 2)
    fdtd.SetBoundaryCond(["MUR"] * 6)
    csx = ContinuousStructure()
    fdtd.SetCSX(csx)
    mesh = csx.GetGrid()
    mesh.SetDeltaUnit(1e-3)

    kappa = 2 * math.pi * f0 * EPS0 * er * tand
    substrate = csx.AddMaterial("substrate", epsilon=er, kappa=kappa)
    cut = csx.AddMaterial("pour_cutout", epsilon=er, kappa=kappa)
    metal = csx.AddMetal("copper")

    bc = board.get("center", {"x": 0, "y": 0})
    if board.get("outline"):
        outline = [(p["x"], p["y"]) for p in board["outline"]]
    else:
        outline = rect(bc["x"], bc["y"], board["width"], board["height"])
    substrate.AddLinPoly(points=[[p[0] for p in outline], [p[1] for p in outline]], norm_dir="z", elevation=0, length=t, priority=0)

    # Copper is a zero-thickness sheet. A pour's holes are the substrate again at a higher priority.
    for layer, outer, holes in pours:
        metal.AddPolygon(points=[[p[0] for p in outer], [p[1] for p in outer]], norm_dir="z", elevation=z[layer], priority=10)
        for hole in holes:
            cut.AddPolygon(points=[[p[0] for p in hole], [p[1] for p in hole]], norm_dir="z", elevation=z[layer], priority=20)
    for layer, poly in copper:
        if layer in z:
            metal.AddPolygon(points=[[p[0] for p in poly], [p[1] for p in poly]], norm_dir="z", elevation=z[layer], priority=30)
    for x, y, r, zs in barrels:
        if len(zs) >= 2 and r > 0:
            metal.AddCylinder(start=[x, y, min(zs)], stop=[x, y, max(zs)], radius=r, priority=30)

    # Mesh: lines on the copper edges near the antenna, then graded out to the air box.
    lam_min = C0 / fmax * 1e3
    res_air = lam_min / (30 if accurate else 20)
    res_sub = res_air / math.sqrt(er)
    tol = 0.05 if accurate else 0.1
    ant_pts = [p for _, poly in copper for p in poly]
    ax0, ax1 = min(p[0] for p in ant_pts), max(p[0] for p in ant_pts)
    ay0, ay1 = min(p[1] for p in ant_pts), max(p[1] for p in ant_pts)
    near = lambda v, lo, hi: lo - 5 <= v <= hi + 5

    def metal_on(layer):
        polys = [poly for l, poly in copper if l == layer]
        fills = [(outer, holes) for l, outer, holes in pours if l == layer]
        return lambda x, y: any(inside(p, x, y) for p in polys) or any(
            inside(o, x, y) and not any(inside(h, x, y) for h in hs) for o, hs in fills
        )

    is_metal = {layer: metal_on(layer) for layer in z}
    edge_res = min(0.3 if accurate else 0.5, res_sub / 2)
    xs, ys = [], []
    for layer, poly in copper:
        ex, ey = thirds(poly, is_metal[layer], edge_res)
        xs += ex
        ys += ey
    for layer, outer, holes in pours:
        for poly in [outer] + holes:
            ex, ey = thirds(poly, is_metal[layer], edge_res)
            xs += [v for v in ex if near(v, ax0, ax1)]
            ys += [v for v in ey if near(v, ay0, ay1)]
    ox, oy = edges(outline)
    xs += ox or [min(p[0] for p in outline), max(p[0] for p in outline)]
    ys += oy or [min(p[1] for p in outline), max(p[1] for p in outline)]

    # The port and every via barrel need a mesh line through them, or FDTD never sees them.
    vias = [(x, y) for x, y, r, zs in barrels if len(zs) >= 2 and r > 0]
    fixed = {round(v, 6) for v in [start[0], stop[0]] + [x for x, _ in vias]}
    xs = merge(xs, fixed, tol)
    fixed = {round(v, 6) for v in [start[1], stop[1]] + [y for _, y in vias]}
    ys = merge(ys, fixed, tol)

    margin = C0 / fmin * 1e3 / 4
    bx0, bx1 = min(p[0] for p in outline), max(p[0] for p in outline)
    by0, by1 = min(p[1] for p in outline), max(p[1] for p in outline)
    mesh.AddLine("x", xs + [bx0 - margin, bx1 + margin])
    mesh.AddLine("y", ys + [by0 - margin, by1 + margin])
    zl = sorted(set(list(z.values()) + list(np.linspace(0, t, 7 if accurate else 5)) + [start[2], stop[2]]))
    mesh.AddLine("z", merge(zl, {round(start[2], 6), round(stop[2], 6)}, tol / 2) + [-margin, t + margin])
    # The board is meshed in the substrate wavelength; above and below it the air is graded coarser.
    mesh.SmoothMeshLines("x", res_sub, 1.4)
    mesh.SmoothMeshLines("y", res_sub, 1.4)
    mesh.SmoothMeshLines("z", res_air, 1.4)

    port = fdtd.AddLumpedPort(1, 50, start, stop, port_dir, 1.0 * sign, priority=5)
    nf2ff = fdtd.CreateNF2FFBox()

    cells = len(mesh.GetLines("x")) * len(mesh.GetLines("y")) * len(mesh.GetLines("z"))
    sim = a["sim_dir"]
    print(f"Mesh {len(mesh.GetLines('x'))} x {len(mesh.GetLines('y'))} x {len(mesh.GetLines('z'))} = {cells} cells", flush=True)
    fdtd.Run(sim, cleanup=True, verbose=0, numThreads=int(a.get("threads", 0)))

    f = np.linspace(fmin, fmax, 401)
    port.CalcPort(sim, f)
    s11 = port.uf_ref / port.uf_inc
    s11_db = 20 * np.log10(np.maximum(np.abs(s11), 1e-12))
    zin = port.uf_tot / port.if_tot
    i_min = int(np.argmin(s11_db))
    band = None
    if s11_db[i_min] < -10:
        lo = hi = i_min
        while lo > 0 and s11_db[lo - 1] < -10:
            lo -= 1
        while hi < len(f) - 1 and s11_db[hi + 1] < -10:
            hi += 1
        band = [float(f[lo]), float(f[hi]), lo == 0 or hi == len(f) - 1]
    i0 = int(np.argmin(abs(f - f0)))

    theta = np.arange(0.0, 181.0, 5.0)
    phi = np.arange(0.0, 360.0, 5.0)
    ff = nf2ff.CalcNF2FF(sim, f0, theta, phi, center=[(bx0 + bx1) / 2 * 1e-3, (by0 + by1) / 2 * 1e-3, t / 2 * 1e-3], read_cached=False)
    p_rad = float(ff.Prad[0])
    port.CalcPort(sim, np.array([f0]))
    p_acc, p_inc = float(port.P_acc[0]), float(port.P_inc[0])
    d_max = float(ff.Dmax[0])
    rad_eff = p_rad / p_acc if p_acc > 0 else 0.0
    tot_eff = p_rad / p_inc if p_inc > 0 else 0.0
    # Gain pattern: directivity scaled by the radiation efficiency.
    u = ff.E_norm[0] ** 2
    gain_db = 10 * np.log10(np.maximum(u / u.max() * d_max * max(rad_eff, 1e-9), 1e-12))
    peak = float(gain_db.max())
    it, ip = np.unravel_index(int(np.argmax(gain_db)), gain_db.shape)

    def cut(ph):
        a_ = list(theta)
        g = list(gain_db[:, int(ph / 5)])
        back = (ph + 180) % 360
        a_ += [360 - th for th in theta[::-1]]
        g += list(gain_db[::-1, int(back / 5)])
        return a_, g

    xz_a, xz_g = cut(0)
    yz_a, yz_g = cut(90)
    xy_g = list(gain_db[int(90 / 5), :])
    xy_a = [90 - p for p in phi]  # compass-style: 0° at +y, clockwise toward +x
    os.makedirs(a["out"], exist_ok=True)
    stem = re.sub(r"[^A-Za-z0-9_-]", "_", ant_name) or "antenna"
    files = [f"{stem}-s11.svg", f"{stem}-pattern.svg", f"{stem}.s1p", f"{stem}.json"]
    with open(os.path.join(a["out"], files[0]), "w") as fh:
        fh.write(s11_svg(f, list(s11_db), f0, f"S11 of {ant_name}"))
    with open(os.path.join(a["out"], files[1]), "w") as fh:
        fh.write(pattern_svg([
            ("XY plane (board)", "#111", xy_a, xy_g),
            ("XZ plane", "#c33", xz_a, xz_g),
            ("YZ plane", "#36c", yz_a, yz_g),
        ], peak, f"{ant_name}: gain at {f0 / 1e6:g} MHz"))
    with open(os.path.join(a["out"], files[2]), "w") as fh:
        fh.write(f"! Forge openEMS simulation of {ant_name}\n# Hz S RI R 50\n")
        for fr, s in zip(f, s11):
            fh.write(f"{fr:.0f} {s.real:.6f} {s.imag:.6f}\n")

    result = {
        "antenna": ant_name,
        "port": port_note,
        "cells": cells,
        "f0": f0,
        "span": [fmin, fmax],
        "resonance": float(f[i_min]),
        "s11_min_db": float(s11_db[i_min]),
        "band": band,
        "s11_f0_db": float(s11_db[i0]),
        "zin_f0": [float(zin[i0].real), float(zin[i0].imag)],
        "zin_res": [float(zin[i_min].real), float(zin[i_min].imag)],
        "directivity_dbi": 10 * math.log10(d_max) if d_max > 0 else None,
        "peak_gain_dbi": peak,
        "peak_dir": [float(theta[it]), float(phi[ip])],
        "radiation_efficiency": rad_eff,
        "total_efficiency": tot_eff,
        "ground_under": grounded,
        "files": files,
    }
    with open(os.path.join(a["out"], files[3]), "w") as fh:
        json.dump(result, fh, indent=2)
    return result


if __name__ == "__main__":
    args = json.loads(sys.argv[1])
    try:
        res = simulate(args)
    except BoardError as e:
        print("FORGE_ERROR " + json.dumps(str(e)), flush=True)
        sys.exit(2)
    print("FORGE_RESULT " + json.dumps(res), flush=True)
