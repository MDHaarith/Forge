"""Makes a dimensioned technical drawing (SVG) of a part, headless, with FreeCAD's TechDraw projection.

Forge launches: freecadcmd freecad_drawing.py with FORGE_PROJECT and FORGE_ARGS (JSON):
  model       project-relative .step/.stp/.brep/.FCStd
  name        output base name: drawings/<name>.svg (and drawings/<name>-top.dxf with dxf=true)
  title, material, projection ("first" | "third"), sheet ("auto" | "A4" | "A3"), dxf
The sheet has front, top and side views with hidden lines, an isometric view, overall
dimensions, a hole table with tags on the views, and an ISO 7200-style title block.
"""

import datetime
import json
import math
import os
import sys
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import FreeCAD as App  # noqa: E402
import Part  # noqa: E402
import TechDraw  # noqa: E402

import forge_cad as forge  # noqa: E402

V = App.Vector


def done(code, line):
    print(line, flush=True)
    sys.stdout.flush()
    os._exit(code)


def fail(message):
    done(1, "FORGE_ERROR " + json.dumps(message))


SHEETS = {"A4": (297.0, 210.0), "A3": (420.0, 297.0), "A2": (594.0, 420.0)}
SCALES = [50, 20, 10, 5, 2, 1, 1 / 2, 1 / 5, 1 / 10, 1 / 20, 1 / 50, 1 / 100]
MARGIN = 10.0
TB_W, TB_H = 120.0, 40.0
GAP = 22.0  # room between views for dimensions
FONT = "DejaVu Sans, Liberation Sans, Arial, sans-serif"


def scale_label(s):
    return f"{round(s)}:1" if s >= 1 else f"1:{round(1 / s)}"


def fmt(x):
    text = f"{x:.2f}".rstrip("0").rstrip(".")
    return "0" if text == "-0" else text


def esc(text):
    return str(text).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


class View:
    """A projection of the part: looking along `facing` with `up` pointing up the sheet."""

    def __init__(self, shape, facing, up, label, hidden=True):
        f = V(facing).normalize()
        u = V(up)
        u = (u - f * u.dot(f)).normalize()
        r = f.cross(u)
        t = f * -1
        self.right, self.up, self.toward = r, u, t
        m = App.Matrix(r.x, r.y, r.z, 0, u.x, u.y, u.z, 0, t.x, t.y, t.z, 0, 0, 0, 0, 1)
        s = shape.copy()
        s.transformShape(m)
        parts = TechDraw.projectEx(s, V(0, 0, 1))
        # projectEx: V, V1 (smooth), VN, VO (outline), VI, H, H1, HN, HO, HI
        self.visible = self._edges(parts[0]) + self._edges(parts[3])
        self.smooth = self._edges(parts[1])
        self.hidden = (self._edges(parts[5]) + self._edges(parts[8])) if hidden else []
        pts = [p for e in self.visible + self.hidden for p in e["pts"]]
        if not pts:
            raise ValueError(f"the {label} view is empty")
        self.xmin, self.xmax = min(p[0] for p in pts), max(p[0] for p in pts)
        self.ymin, self.ymax = min(p[1] for p in pts), max(p[1] for p in pts)
        self.w, self.h = self.xmax - self.xmin, self.ymax - self.ymin
        self.label = label
        self.ox = self.oy = 0.0
        self.scale = 1.0

    @staticmethod
    def _edges(compound):
        out = []
        if compound is None or compound.isNull():
            return out
        for e in compound.Edges:
            if e.Length < 1e-6:
                continue
            if isinstance(e.Curve, Part.Line):
                pts = [e.Vertexes[0].Point, e.Vertexes[-1].Point]
            else:
                n = max(8, min(256, int(e.Length / 0.5)))
                pts = e.discretize(n)
            out.append({"edge": e, "pts": [(p.x, p.y) for p in pts]})
        return out

    def to_view(self, p):
        """Model point -> view coordinates (before placement on the sheet)."""
        p = V(*p) if not isinstance(p, App.Vector) else p
        return p.dot(self.right), p.dot(self.up)

    def sheet(self, x, y):
        """View coordinates -> sheet millimetres (y down)."""
        return self.ox + (x - self.xmin) * self.scale, self.oy - (y - self.ymin) * self.scale

    def place(self, left, bottom, scale):
        self.ox, self.oy, self.scale = left, bottom, scale

    @property
    def box(self):
        """Sheet rectangle: left, top, right, bottom."""
        return self.ox, self.oy - self.h * self.scale, self.ox + self.w * self.scale, self.oy


class Svg:
    def __init__(self, w, h):
        self.w, self.h = w, h
        self.items = []

    def add(self, text):
        self.items.append(text)

    def path(self, d, cls):
        self.add(f'<path class="{cls}" d="{d}"/>')

    def line(self, a, b, cls):
        self.add(f'<line class="{cls}" x1="{a[0]:.3f}" y1="{a[1]:.3f}" x2="{b[0]:.3f}" y2="{b[1]:.3f}"/>')

    def text(self, x, y, s, size=3.5, anchor="middle", rotate=None, weight=None):
        rot = f' transform="rotate({rotate} {x:.3f} {y:.3f})"' if rotate else ""
        bold = f' font-weight="{weight}"' if weight else ""
        self.add(f'<text x="{x:.3f}" y="{y:.3f}" font-size="{size}" text-anchor="{anchor}"{bold}{rot}>{esc(s)}</text>')

    def render(self):
        style = (
            "path,line,rect,circle,polygon{fill:none;stroke:#000;stroke-linecap:round;stroke-linejoin:round}"
            ".vis{stroke-width:0.5}.smooth{stroke-width:0.18}.hid{stroke-width:0.25;stroke-dasharray:1.6 0.8}"
            ".thin{stroke-width:0.18}.center{stroke-width:0.18;stroke-dasharray:6 1.2 1 1.2}"
            ".frame{stroke-width:0.7}.arrow{fill:#000;stroke:none}"
            f"text{{font-family:{FONT};fill:#000;stroke:none}}"
        )
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.w:g}mm" height="{self.h:g}mm" viewBox="0 0 {self.w:g} {self.h:g}">\n'
            f"<style>{style}</style>\n"
            f'<rect x="0" y="0" width="{self.w:g}" height="{self.h:g}" fill="#fff" stroke="none"/>\n' + "\n".join(self.items) + "\n</svg>\n"
        )


def draw_view(svg, view):
    for cls, edges in (("hid", view.hidden), ("smooth", view.smooth), ("vis", view.visible)):
        for e in edges:
            pts = [view.sheet(*p) for p in e["pts"]]
            svg.path("M" + " L".join(f"{x:.3f} {y:.3f}" for x, y in pts), cls)


def arrow(svg, tip, toward):
    """A filled arrowhead at `tip`, pointing at tip from `toward`."""
    dx, dy = tip[0] - toward[0], tip[1] - toward[1]
    n = math.hypot(dx, dy) or 1
    ux, uy = dx / n, dy / n
    a = (tip[0] - ux * 3 - uy * 0.6, tip[1] - uy * 3 + ux * 0.6)
    b = (tip[0] - ux * 3 + uy * 0.6, tip[1] - uy * 3 - ux * 0.6)
    svg.add(f'<polygon class="arrow" points="{tip[0]:.3f},{tip[1]:.3f} {a[0]:.3f},{a[1]:.3f} {b[0]:.3f},{b[1]:.3f}"/>')


def dim_h(svg, x1, x2, y_ref, y_dim, value):
    """Horizontal dimension between x1 and x2 measured at y_ref, drawn at y_dim."""
    sign = 1 if y_dim > y_ref else -1
    for x in (x1, x2):
        svg.line((x, y_ref + sign * 1.5), (x, y_dim + sign * 2), "thin")
    svg.line((x1, y_dim), (x2, y_dim), "thin")
    arrow(svg, (x1, y_dim), (x2, y_dim))
    arrow(svg, (x2, y_dim), (x1, y_dim))
    svg.text((x1 + x2) / 2, y_dim - 1.2, fmt(value))


def dim_v(svg, y1, y2, x_ref, x_dim, value):
    sign = 1 if x_dim > x_ref else -1
    for y in (y1, y2):
        svg.line((x_ref + sign * 1.5, y), (x_dim + sign * 2, y), "thin")
    svg.line((x_dim, y1), (x_dim, y2), "thin")
    arrow(svg, (x_dim, y1), (x_dim, y2))
    arrow(svg, (x_dim, y2), (x_dim, y1))
    svg.text(x_dim - 1.2, (y1 + y2) / 2, fmt(value), rotate=-90)


def centre_mark(svg, view, center, radius):
    cx, cy = view.sheet(*view.to_view(center))
    r = radius * view.scale + 1.5
    svg.line((cx - r, cy), (cx + r, cy), "center")
    svg.line((cx, cy - r), (cx, cy + r), "center")


def projection_symbol(svg, x, y, first):
    """ISO 5456-2 projection symbol, about 14 × 6 mm, with its left edge at x and centre line at y."""
    cone_x, circ_x = (x, x + 11) if first else (x + 7, x)
    cx = circ_x + 2.5
    svg.add(f'<circle class="thin" cx="{cx:.2f}" cy="{y:.2f}" r="2.5"/><circle class="thin" cx="{cx:.2f}" cy="{y:.2f}" r="1.2"/>')
    pts = [(cone_x, y - 1.2), (cone_x + 7, y - 2.5), (cone_x + 7, y + 2.5), (cone_x, y + 1.2)]
    svg.add('<polygon class="thin" points="' + " ".join(f"{a:.2f},{b:.2f}" for a, b in pts) + '"/>')


def main():
    project = os.environ["FORGE_PROJECT"]
    args = json.loads(os.environ["FORGE_ARGS"])
    os.chdir(project)
    name = args["name"]
    shape = forge.load(args["model"])
    if not shape.Solids:
        fail(f"{args['model']} has no solids to draw")
    first = args.get("projection", "first") != "third"
    material = args.get("material")
    title = args.get("title") or name

    bb = shape.BoundBox
    front = View(shape, (0, 1, 0), (0, 0, 1), "FRONT")
    top = View(shape, (0, 0, -1), (0, 1, 0), "TOP")
    # First angle puts the view from the left on the right of the front view; third angle, the view from the right.
    side = View(shape, (1, 0, 0) if first else (-1, 0, 0), (0, 0, 1), "LEFT" if first else "RIGHT")
    iso = View(shape, (-1, 1, -1), (0, 0, 1), "ISOMETRIC", hidden=False)

    # The smallest sheet that shows the part at full size or larger; otherwise A3 at whatever scale fits.
    def fit(sheet_name):
        sw, sh = SHEETS[sheet_name]
        avail_w = sw - 2 * MARGIN - TB_W - 3 * GAP
        avail_h = sh - 2 * MARGIN - 3 * GAP
        return next((s for s in SCALES if s * (front.w + side.w) <= avail_w and s * (front.h + top.h) <= avail_h), None)

    wanted = args.get("sheet", "auto")
    if wanted in SHEETS:
        best = (wanted, fit(wanted))
    else:
        best = next(((n, fit(n)) for n in ("A4", "A3") if (fit(n) or 0) >= 1), ("A3", fit("A3")))
        if best[1] is None:
            best = ("A2", fit("A2"))
    if best[1] is None:
        fail(f"the part is too large to draw on an {best[0]} sheet at 1:100")
    sheet_name, s = best
    sw, sh = SHEETS[sheet_name]

    # Lay the views out in a grid at the left; isometric above the title block on the right.
    grid_w = s * (front.w + side.w) + GAP
    grid_h = s * (front.h + top.h) + GAP
    left = MARGIN + GAP + (sw - 2 * MARGIN - TB_W - 3 * GAP - grid_w) / 2
    top_y = MARGIN + GAP + (sh - 2 * MARGIN - 3 * GAP - grid_h) / 2
    upper_h = s * (front.h if first else top.h)
    front_bottom = top_y + (upper_h if first else s * top.h + GAP + s * front.h)
    front.place(left, front_bottom, s)
    side.place(left + s * front.w + GAP, front_bottom, s)
    top.place(left, front_bottom + GAP + s * top.h if first else top_y + s * top.h, s)

    iso_area_w, iso_area_h = TB_W - 10, sh - 2 * MARGIN - TB_H - 20
    iso_s = min(s, iso_area_w / max(iso.w, 1e-6), iso_area_h / max(iso.h, 1e-6))
    iso_left = sw - MARGIN - TB_W + (TB_W - iso.w * iso_s) / 2
    iso_bottom = MARGIN + 10 + (iso_area_h + iso.h * iso_s) / 2
    iso.place(iso_left, iso_bottom, iso_s)

    svg = Svg(sw, sh)
    svg.add(f'<rect class="frame" x="{MARGIN}" y="{MARGIN}" width="{sw - 2 * MARGIN}" height="{sh - 2 * MARGIN}"/>')
    for v in (front, top, side, iso):
        draw_view(svg, v)
        l, t, r, b = v.box
        label = v.label + (" (reduced)" if v is iso and iso_s < s - 1e-9 else "")
        svg.text((l + r) / 2, b + 7 if v is not iso else b + 6, label, size=2.5)

    # Overall dimensions: width and height on the front view, depth on the top view.
    l, t, r, b = front.box
    dim_h(svg, l, r, t, t - 8, bb.XLength)
    dim_v(svg, t, b, l, l - 8, bb.ZLength)
    l2, t2, r2, b2 = top.box
    dim_v(svg, t2, b2, l2, l2 - 8, bb.YLength)

    # Holes: tag them on the view that looks down their axis, and list them in a table.
    found = forge.holes_in(shape)
    rows = []
    looks = {"Z": top, "Y": front, "X": side}
    # Hole outlines on the sheet, so tags are put where they cover no hole and no other tag.
    circles = []
    for h in found:
        view = looks.get(h["axis"])
        if view is not None:
            cx, cy = view.sheet(*view.to_view(V(*h["center"])))
            r = h.get("counterbore", [h.get("countersink") or h["diameter"]])[0] / 2 * view.scale
            circles.append((view, cx, cy, r))
    taken = []

    def clashes(box, view):
        x0, y0, x1, y1 = box
        l, t, r, b = view.box
        # A tag either side of the view's outline is readable; one on the outline is not.
        if (x0 < l < x1 or x0 < r < x1) and y1 > t and y0 < b or (y0 < t < y1 or y0 < b < y1) and x1 > l and x0 < r:
            return True
        for _, cx, cy, r in circles:
            nx, ny = min(max(cx, x0), x1), min(max(cy, y0), y1)
            if (nx - cx) ** 2 + (ny - cy) ** 2 < (r + 0.6) ** 2:
                return True
        return any(x0 < b[2] and b[0] < x1 and y0 < b[3] and b[1] < y1 for b in taken)

    for i, h in enumerate(found, 1):
        tag = f"H{i}"
        view = looks.get(h["axis"])
        c = V(*h["center"])
        if view is not None:
            centre_mark(svg, view, c, h["diameter"] / 2)
            cx, cy = view.sheet(*view.to_view(c))
            off = h.get("counterbore", [h.get("countersink") or h["diameter"]])[0] / 2 * view.scale
            tw, th = 1.7 * len(tag) + 0.6, 2.6
            options = []
            for reach in (3.0, 6.0, 10.0):
                for dx, dy in ((1, -1), (-1, -1), (1, 1), (-1, 1), (1, 0), (-1, 0), (0, -1), (0, 1)):
                    n = math.hypot(dx, dy)
                    ux, uy = dx / n, dy / n
                    ex, ey = cx + ux * (off + reach), cy + uy * (off + reach)
                    lx = ex + 0.5 if dx > 0 else ex - tw - 0.5 if dx < 0 else ex - tw / 2
                    ly = ey - 0.4 - th if dy < 0 else ey + 0.4 if dy > 0 else ey - th / 2
                    options.append(((cx + ux * off * 0.7, cy + uy * off * 0.7), (ex, ey), (lx, ly, lx + tw, ly + th)))
            start, end, box = next((o for o in options if not clashes(o[2], view)), options[0])
            taken.append(box)
            svg.line(start, end, "thin")
            svg.text(box[0], box[3] - 0.45, tag, size=2.5, anchor="start")
        desc = f"Ø{fmt(h['diameter'])} " + ("THRU" if h["through"] else f"↧{fmt(h['depth'])}")
        if "counterbore" in h:
            desc += f"  CBORE Ø{fmt(h['counterbore'][0])} ↧{fmt(h['counterbore'][1])}"
        if h.get("countersink"):
            desc += f"  CSK Ø{fmt(h['countersink'])}×90°"
        rel = [c.x - bb.XMin, c.y - bb.YMin, c.z - bb.ZMin]
        at = {"Z": f"X {fmt(rel[0])}  Y {fmt(rel[1])}", "Y": f"X {fmt(rel[0])}  Z {fmt(rel[2])}", "X": f"Y {fmt(rel[1])}  Z {fmt(rel[2])}"}.get(h["axis"], "")
        rows.append((tag, desc, h["axis"], at, h.get("fits") or ""))

    tb_left, tb_top = sw - MARGIN - TB_W, sh - MARGIN - TB_H
    if rows:
        shown = rows[:14]
        row_h = 4.5
        table_h = row_h * (len(shown) + 1)
        tx, ty = MARGIN + 4, sh - MARGIN - 4 - table_h
        # Keep the table clear of the views; if it would overlap, put it above the title block instead.
        lowest = max(v.box[3] for v in (front, top, side)) + 9
        if ty < lowest:
            tx, ty = tb_left, tb_top - 4 - table_h
        heads = ["TAG", "HOLE", "AXIS", "POSITION", "FOR"]
        # Each column as wide as its longest text at 2.3 mm (about 1.35 mm a character).
        cols = [round(max(len(r[k]) for r in [heads] + shown) * 1.35 + 2.6, 1) for k in range(5)]
        svg.add(f'<rect class="thin" x="{tx:.2f}" y="{ty:.2f}" width="{sum(cols)}" height="{table_h:.2f}"/>')
        for j, row in enumerate([heads] + shown):
            y = ty + row_h * (j + 1)
            if j < len(shown):
                svg.line((tx, y), (tx + sum(cols), y), "thin")
            x = tx
            for k, cell in enumerate(row):
                svg.text(x + 1.2, y - 1.3, cell, size=2.3, anchor="start", weight="bold" if j == 0 else None)
                x += cols[k]
        x = tx
        for wcol in cols[:-1]:
            x += wcol
            svg.line((x, ty), (x, ty + table_h), "thin")
        if len(rows) > len(shown):
            svg.text(tx, ty - 1.5, f"+{len(rows) - len(shown)} more holes: see freecad_inspect", size=2.3, anchor="start")
        svg.text(tx, ty - (5 if len(rows) > len(shown) else 1.5), "Hole positions from the part's lower-left corner (X min, Y min, Z min)", size=2.2, anchor="start")

    # Title block
    mass = f"{forge.mass_g(shape, material):.1f} g" if material else "—"
    svg.add(f'<rect class="frame" x="{tb_left}" y="{tb_top}" width="{TB_W}" height="{TB_H}"/>')
    for y in (10, 20, 30):
        svg.line((tb_left, tb_top + y), (tb_left + TB_W, tb_top + y), "thin")
    svg.line((tb_left + 60, tb_top + 10), (tb_left + 60, tb_top + TB_H), "thin")
    svg.line((tb_left + 90, tb_top + 20), (tb_left + 90, tb_top + TB_H), "thin")

    def cell(x, y, label, value, size=3.0):
        svg.text(tb_left + x + 1.5, tb_top + y + 3, label, size=1.9, anchor="start")
        svg.text(tb_left + x + 1.5, tb_top + y + 8.2, value, size=size, anchor="start")

    cell(0, 0, "TITLE", title, 4.2)
    cell(0, 10, "PART / FILE", f"{name}  ({os.path.basename(args['model'])})", 2.6)
    cell(60, 10, "MATERIAL", material or "—", 2.8)
    cell(0, 20, "UNITS / GENERAL TOLERANCE", "mm   ISO 2768-m", 2.8)
    cell(60, 20, "MASS", mass, 2.8)
    cell(90, 20, "SCALE", scale_label(s), 2.8)
    cell(0, 30, "DRAWN", f"Forge   {datetime.date.today().isoformat()}", 2.8)
    cell(60, 30, "SHEET", sheet_name, 2.8)
    svg.text(tb_left + 91.5, tb_top + 33, "PROJECTION", size=1.9, anchor="start")
    projection_symbol(svg, tb_left + 93, tb_top + 36.5, first)

    os.makedirs("drawings", exist_ok=True)
    out = f"drawings/{name}.svg"
    with open(out, "w", encoding="utf-8") as f:
        f.write(svg.render())
    files = [out]

    if args.get("dxf"):
        dxf = f"drawings/{name}-top.dxf"
        write_dxf(dxf, top)
        files.append(dxf)

    done(
        0,
        "FORGE_RESULT "
        + json.dumps(
            {
                "files": files,
                "sheet": sheet_name,
                "scale": scale_label(s),
                "projection": "first angle" if first else "third angle",
                "size_mm": [round(bb.XLength, 3), round(bb.YLength, 3), round(bb.ZLength, 3)],
                "holes": [{"tag": r[0], "hole": r[1], "axis": r[2], "at": r[3], "for": r[4]} for r in rows],
                "mass_g": round(forge.mass_g(shape, material), 2) if material else None,
            }
        ),
    )


def write_dxf(path, view):
    """The top view's visible edges at 1:1 as DXF R12: true arcs and circles for CNC and laser cutting."""
    out = ["0", "SECTION", "2", "ENTITIES"]

    def add(kind, *pairs):
        out.extend(["0", kind, "8", "0"])
        for code, value in pairs:
            out.extend([str(code), f"{value:.6f}" if isinstance(value, float) else str(value)])

    for item in view.visible:
        e = item["edge"]
        if isinstance(e.Curve, Part.Line):
            a, b = e.Vertexes[0].Point, e.Vertexes[-1].Point
            add("LINE", (10, a.x), (20, a.y), (30, 0.0), (11, b.x), (21, b.y), (31, 0.0))
        elif isinstance(e.Curve, Part.Circle):
            c, r = e.Curve.Center, e.Curve.Radius
            if e.isClosed():
                add("CIRCLE", (10, c.x), (20, c.y), (30, 0.0), (40, r))
            else:
                ang = lambda p: math.degrees(math.atan2(p.y - c.y, p.x - c.x)) % 360  # noqa: E731
                a1, a2 = ang(e.Vertexes[0].Point), ang(e.Vertexes[-1].Point)
                mid = ang(e.valueAt((e.FirstParameter + e.LastParameter) / 2))
                # DXF arcs run counter-clockwise from start to end.
                if not ((mid - a1) % 360 < (a2 - a1) % 360):
                    a1, a2 = a2, a1
                add("ARC", (10, c.x), (20, c.y), (30, 0.0), (40, r), (50, a1), (51, a2))
        else:
            pts = item["pts"]
            for (x1, y1), (x2, y2) in zip(pts, pts[1:]):
                add("LINE", (10, x1), (20, y1), (30, 0.0), (11, x2), (21, y2), (31, 0.0))
    out.extend(["0", "ENDSEC", "0", "EOF"])
    with open(path, "w", encoding="ascii") as f:
        f.write("\n".join(out) + "\n")


try:
    main()
except BaseException:
    fail("the drawing failed:\n" + traceback.format_exc(limit=8))
