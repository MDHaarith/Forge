"""Engineering helpers for Forge's FreeCAD scripts, available in every script as `forge`.

Units are millimetres. Holes are cut along -Z from the top of the part unless `top` is given.

    plate = forge.rounded_box(60, 40, 5, radius=4)
    plate = forge.holes(plate, [(6, 6), (54, 6), (6, 34), (54, 34)], "M3", kind="counterbore")
    box = forge.enclosure(80, 50, 30, wall=2, floor=2, radius=3)
    box = forge.standoffs(box, [(10, 10), (70, 40)], "M3", height=6, base=2)
    board = forge.place(forge.load("models/pcb.step"), z=8)

Numbers come from ISO 273 (clearance holes), ISO 261/724 coarse threads (tap drills),
ISO 4762 (socket head cap screws, counterbores) and ISO 10642 (countersunk screws).
Heat-set insert sizes are typical for brass inserts; check your insert's datasheet.
"""

import math
import os

import FreeCAD as App
import Part

V = App.Vector

# ISO 273 clearance holes: size -> (close, normal, loose)
CLEARANCE = {
    "M1.6": (1.7, 1.8, 2.0),
    "M2": (2.2, 2.4, 2.6),
    "M2.5": (2.7, 2.9, 3.1),
    "M3": (3.2, 3.4, 3.6),
    "M4": (4.3, 4.5, 4.8),
    "M5": (5.3, 5.5, 5.8),
    "M6": (6.4, 6.6, 7.0),
    "M8": (8.4, 9.0, 10.0),
    "M10": (10.5, 11.0, 12.0),
    "M12": (13.0, 13.5, 14.5),
}
FITS = {"close": 0, "normal": 1, "loose": 2}

# Tap drill for coarse threads
TAP_DRILL = {"M1.6": 1.25, "M2": 1.6, "M2.5": 2.05, "M3": 2.5, "M4": 3.3, "M5": 4.2, "M6": 5.0, "M8": 6.8, "M10": 8.5, "M12": 10.2}

# ISO 4762 socket head cap screw head: size -> (head diameter, head height)
SOCKET_HEAD = {"M2": (3.8, 2.0), "M2.5": (4.5, 2.5), "M3": (5.5, 3.0), "M4": (7.0, 4.0), "M5": (8.5, 5.0), "M6": (10.0, 6.0), "M8": (13.0, 8.0), "M10": (16.0, 10.0), "M12": (18.0, 12.0)}

# ISO 10642 countersunk head diameter (90 degree countersink)
COUNTERSUNK_HEAD = {"M3": 6.72, "M4": 8.96, "M5": 11.2, "M6": 13.44, "M8": 17.92, "M10": 22.4, "M12": 26.88}

# Brass heat-set inserts for 3D prints: size -> (hole diameter, minimum hole depth)
INSERT = {"M2": (3.2, 4.0), "M2.5": (3.6, 5.7), "M3": (4.0, 5.7), "M4": (5.6, 8.1), "M5": (6.4, 9.5)}

# Density in g/cm³
MATERIALS = {
    "PLA": 1.24,
    "PETG": 1.27,
    "ABS": 1.04,
    "ASA": 1.07,
    "Nylon PA12": 1.01,
    "Polycarbonate": 1.20,
    "Acrylic": 1.18,
    "POM (Delrin)": 1.41,
    "Aluminium 6061": 2.70,
    "Aluminium 7075": 2.81,
    "Steel": 7.85,
    "Stainless 304": 8.00,
    "Brass": 8.50,
    "Copper": 8.96,
    "Titanium Ti-6Al-4V": 4.43,
    "Plywood": 0.60,
}


def _size(size):
    size = str(size).upper().replace(" ", "")
    return size if size.startswith("M") else "M" + size


def _lookup(table, size, what):
    key = _size(size)
    if key not in table:
        raise ValueError(f"no {what} data for {key}; known sizes: {', '.join(table)}")
    return table[key]


def clearance(size, fit="normal"):
    """Clearance hole diameter for a metric screw (ISO 273): fit is close, normal or loose."""
    return _lookup(CLEARANCE, size, "clearance hole")[FITS[fit]]


def tap_drill(size):
    """Drill diameter for tapping a coarse metric thread."""
    return _lookup(TAP_DRILL, size, "tap drill")


def density(material):
    """Density in g/cm³ for a material name from MATERIALS (case-insensitive)."""
    for name, rho in MATERIALS.items():
        if name.lower() == str(material).lower() or name.lower().split(" ")[0] == str(material).lower():
            return rho
    raise ValueError(f"unknown material {material!r}; known: {', '.join(MATERIALS)}")


def hole(shape, x, y, size="M3", kind="clearance", depth=None, fit="normal", top=None):
    """Cuts one hole down from z=top (default: the top of the part) at (x, y).

    kind: clearance (screw passes through), tap (to be threaded), counterbore (socket head
    sits flush), countersink (countersunk head sits flush), insert (heat-set insert),
    or a number for a plain hole of that diameter. depth=None drills through.
    """
    box = shape.BoundBox
    top = box.ZMax if top is None else top
    through = box.ZLength + 2
    if isinstance(kind, (int, float)):
        diameter = float(kind)
    elif kind in ("clearance", "counterbore", "countersink"):
        diameter = clearance(size, fit)
    elif kind == "tap":
        diameter = tap_drill(size)
    elif kind == "insert":
        diameter, min_depth = _lookup(INSERT, size, "heat-set insert")
        depth = depth if depth is not None else min_depth + 0.5
    else:
        raise ValueError(f"unknown hole kind {kind!r}")
    length = through if depth is None else depth
    # Start 1 mm above the surface so the cut cleanly opens the top face.
    tool = Part.makeCylinder(diameter / 2, length + 1, V(x, y, top - length), V(0, 0, 1))
    if kind == "counterbore":
        head, height = _lookup(SOCKET_HEAD, size, "socket head")
        bore = head + (1.0 if head < 13 else 2.0)
        tool = tool.fuse(Part.makeCylinder(bore / 2, height + 0.3 + 1, V(x, y, top - height - 0.3)))
    elif kind == "countersink":
        head = _lookup(COUNTERSUNK_HEAD, size, "countersunk head")
        sink = (head - diameter) / 2  # 90 degree cone: depth equals radial step
        tool = tool.fuse(Part.makeCone(diameter / 2, head / 2 + 1, sink + 1, V(x, y, top - sink)))
    return shape.cut(tool)


def holes(shape, points, size="M3", **options):
    """Cuts the same hole at every (x, y) in points; options as for hole()."""
    for x, y in points:
        shape = hole(shape, x, y, size, **options)
    return shape


def grid(x0, y0, dx, dy, nx=2, ny=2):
    """Points of an nx × ny pattern starting at (x0, y0) with pitch dx, dy."""
    return [(x0 + i * dx, y0 + j * dy) for j in range(ny) for i in range(nx)]


def corners(length, width, inset):
    """The four points `inset` in from the corners of a length × width rectangle at the origin."""
    return [(inset, inset), (length - inset, inset), (length - inset, width - inset), (inset, width - inset)]


def vertical_edges(shape):
    """Straight edges parallel to Z (the corners of a box, ready for fillet())."""
    found = []
    for e in shape.Edges:
        if isinstance(e.Curve, Part.Line):
            a, b = e.Vertexes[0].Point, e.Vertexes[-1].Point
            if abs(a.x - b.x) < 1e-6 and abs(a.y - b.y) < 1e-6:
                found.append(e)
    return found


def edges_at(shape, z, tol=1e-6):
    """Edges lying entirely at height z (e.g. the top rim, for a chamfer)."""
    return [e for e in shape.Edges if all(abs(v.Point.z - z) < tol for v in e.Vertexes) and abs(e.BoundBox.ZLength) < tol]


def fillet(shape, radius, edges=None):
    """Rounds the given edges (default: all of them)."""
    return shape.makeFillet(radius, edges if edges is not None else shape.Edges)


def chamfer(shape, size, edges=None):
    """Bevels the given edges (default: all of them)."""
    return shape.makeChamfer(size, edges if edges is not None else shape.Edges)


def rounded_box(length, width, height, radius=0, x=0, y=0, z=0):
    """A box from (x, y, z) with its vertical edges rounded to `radius`."""
    box = Part.makeBox(length, width, height, V(x, y, z))
    return fillet(box, radius, vertical_edges(box)) if radius > 0 else box


def enclosure(length, width, height, wall=2.0, floor=2.0, radius=0):
    """An open-top box: outside length × width × height, walls `wall` thick, base `floor` thick."""
    outer = rounded_box(length, width, height, radius)
    inner = rounded_box(length - 2 * wall, width - 2 * wall, height, max(radius - wall, 0), wall, wall, floor)
    return outer.cut(inner)


def lid(length, width, thickness=2.0, wall=2.0, lip=2.0, clearance_gap=0.2, radius=0):
    """A lid for enclosure(): a plate with a lip that fits inside the walls with a small gap."""
    plate = rounded_box(length, width, thickness, radius)
    inset = wall + clearance_gap
    lip_shape = rounded_box(length - 2 * inset, width - 2 * inset, lip, max(radius - inset, 0), inset, inset, -lip)
    return plate.fuse(lip_shape).removeSplitter()


def standoffs(shape, points, size="M3", height=5.0, base=0.0, outer=None, kind="insert"):
    """Adds round bosses of `height` on top of z=base at each point, with a hole for the screw.

    kind: insert (heat-set insert, for prints), tap (thread the plastic or metal), or clearance.
    """
    if kind == "insert":
        hole_d = _lookup(INSERT, size, "heat-set insert")[0]
    elif kind == "tap":
        hole_d = tap_drill(size)
    else:
        hole_d = clearance(size)
    outer = outer or max(hole_d * 2, hole_d + 3)
    for x, y in points:
        shape = shape.fuse(Part.makeCylinder(outer / 2, height, V(x, y, base)))
    shape = shape.removeSplitter()
    depth = min(height, _lookup(INSERT, size, "heat-set insert")[1] + 0.5) if kind == "insert" else height
    for x, y in points:
        shape = shape.cut(Part.makeCylinder(hole_d / 2, depth + 1, V(x, y, base + height - depth)))
    return shape


def place(shape, x=0, y=0, z=0, rx=0, ry=0, rz=0):
    """A copy of shape rotated (degrees, about X then Y then Z through the origin) and moved by (x, y, z)."""
    s = shape.copy()
    for angle, axis in ((rx, V(1, 0, 0)), (ry, V(0, 1, 0)), (rz, V(0, 0, 1))):
        if angle:
            s.rotate(V(0, 0, 0), axis, angle)
    s.translate(V(x, y, z))
    return s


def load(path):
    """Reads a part from a project file: .step/.stp/.iges/.brep, or the solids of an .FCStd."""
    full = os.path.abspath(path)
    if not os.path.isfile(full):
        raise FileNotFoundError(f"{path} does not exist")
    if full.lower().endswith(".fcstd"):
        doc = App.openDocument(full, True)
        try:
            tops = [o.Shape for o in doc.Objects if hasattr(o, "Shape") and not o.InList and o.Shape.Solids]
            if not tops:
                raise ValueError(f"{path} has no solids")
            return tops[0].copy() if len(tops) == 1 else Part.makeCompound([s.copy() for s in tops])
        finally:
            App.closeDocument(doc.Name)
    shape = Part.Shape()
    shape.read(full)
    return shape


def holes_in(shape, tol=1e-3):
    """Finds the drilled features: coaxial cylinders (and cones) that cut into the part.

    Returns dicts sorted by position: axis ('X', 'Y', 'Z' or 'other'), diameter, through,
    depth, counterbore (diameter, depth) or countersink diameter, center (entry point).
    """
    groups = {}
    for face in shape.Faces:
        surf = face.Surface
        if not isinstance(surf, (Part.Cylinder, Part.Cone)):
            continue
        axis = V(surf.Axis).normalize()
        if axis.z < -tol or (abs(axis.z) < tol and (axis.y < -tol or (abs(axis.y) < tol and axis.x < 0))):
            axis = axis * -1
        # A hole's surface faces its axis: the outward normal points inwards.
        u0, u1, v0, v1 = face.ParameterRange
        point = face.valueAt((u0 + u1) / 2, (v0 + v1) / 2)
        normal = face.normalAt((u0 + u1) / 2, (v0 + v1) / 2)
        foot = surf.Center + axis * (point - surf.Center).dot(axis) if isinstance(surf, Part.Cylinder) else surf.Apex + axis * (point - surf.Apex).dot(axis)
        radial = point - foot
        if radial.Length < tol or normal.dot(radial) > 0:
            continue
        origin = surf.Center if isinstance(surf, Part.Cylinder) else surf.Apex
        across = origin - axis * origin.dot(axis)
        key = (round(axis.x, 3), round(axis.y, 3), round(axis.z, 3), round(across.x, 2), round(across.y, 2), round(across.z, 2))
        lo = min(v.Point.dot(axis) for v in face.Vertexes) if face.Vertexes else origin.dot(axis)
        hi = max(v.Point.dot(axis) for v in face.Vertexes) if face.Vertexes else origin.dot(axis)
        span = [p.dot(axis) for p in (face.BoundBox.getPoint(i) for i in range(8))]
        lo, hi = min(lo, *span), max(hi, *span)
        if isinstance(surf, Part.Cylinder):
            groups.setdefault(key, {"axis": axis, "across": across, "cyl": [], "cone": []})["cyl"].append((surf.Radius * 2, lo, hi, face, u1 - u0))
        else:
            groups.setdefault(key, {"axis": axis, "across": across, "cyl": [], "cone": []})["cone"].append((lo, hi, face))

    found = []
    for g in groups.values():
        if not g["cyl"]:
            continue
        axis, across = g["axis"], g["across"]
        cyls = sorted(g["cyl"], key=lambda c: c[0])
        # A hole goes all the way round; a rounded inside corner of a pocket is only part of a circle.
        if sum(c[4] for c in cyls if abs(c[0] - cyls[0][0]) < tol) < 2 * math.pi - 1e-3:
            continue
        diameter = round(cyls[0][0], 3)
        hi = max([c[2] for c in g["cyl"]] + [c[1] for c in g["cone"]])
        bore_lo = min(c[1] for c in cyls if abs(c[0] - cyls[0][0]) < tol)
        eps = 0.05
        open_top = not shape.isInside(across + axis * (hi + eps), 1e-4, False)
        open_bottom = not shape.isInside(across + axis * (bore_lo - eps), 1e-4, False)
        through = open_top and open_bottom
        # The entry is the open end; a hole drilled from below enters at its low end.
        entry = across + axis * (hi if open_top or not open_bottom else bore_lo)
        depth = round(hi - bore_lo, 3)
        hole = {
            "axis": "Z" if abs(axis.z) > 1 - tol else "Y" if abs(axis.y) > 1 - tol else "X" if abs(axis.x) > 1 - tol else "other",
            "diameter": diameter,
            "through": through,
            "depth": depth,
            "center": [round(entry.x, 3), round(entry.y, 3), round(entry.z, 3)],
        }
        bigger = [c for c in cyls if c[0] > cyls[0][0] + tol]
        if bigger:
            d, blo = bigger[-1][:2]
            # Measured from the surface, so a chamfer at the mouth does not shorten it.
            hole["counterbore"] = [round(d, 3), round(hi - blo, 3)]
        for _, _, cface in g["cone"]:
            circles = [e.Curve.Radius * 2 for e in cface.Edges if isinstance(e.Curve, Part.Circle)]
            if not circles:
                continue
            small, big = min(circles), max(circles)
            # A countersink starts at the bore and is wide enough for a screw head; a narrower
            # cone is an edge break (chamfer), which is not a feature of the hole.
            if abs(small - diameter) < 0.05 and (big - small) / 2 >= 0.25 * diameter:
                hole["countersink"] = round(big, 3)
        hole["fits"] = describe_diameter(diameter, through)
        found.append(hole)
    found.sort(key=lambda h: (h["axis"], -h["center"][2], h["center"][1], h["center"][0]))
    return found


def describe_diameter(d, through=True, tol=0.02):
    """What a hole diameter is for, if it matches a standard size (e.g. 'M3 clearance').

    Some sizes mean two things (3.6 mm is a loose M3 clearance and an M2.5 insert hole), so a
    blind hole is matched to inserts before clearances.
    """
    def tap():
        return next((f"{size} tap" for size, drill in TAP_DRILL.items() if abs(drill - d) < tol), None)

    def insert():
        return next((f"{size} insert" for size, (hole_d, _) in INSERT.items() if abs(hole_d - d) < tol), None)

    def clear():
        for size, fits in CLEARANCE.items():
            for fit, i in FITS.items():
                if abs(fits[i] - d) < tol:
                    return f"{size} clearance" + ("" if fit == "normal" else f" ({fit})")
        return None

    for match in (tap, clear, insert) if through else (tap, insert, clear):
        found = match()
        if found:
            return found
    return None


def mass_g(shape, material):
    """Mass in grams of shape made from material."""
    return shape.Volume / 1000.0 * density(material)


def overhangs(shape, angle=45.0, deflection=0.1):
    """Downward-facing area (mm²) steeper than `angle` from vertical, excluding the bed (lowest) faces.

    For FDM printing in the current orientation: large values need supports.
    """
    points, triangles = shape.tessellate(deflection)
    zmin = shape.BoundBox.ZMin
    limit = -math.sin(math.radians(angle))
    area = 0.0
    for a, b, c in triangles:
        p, q, r = points[a], points[b], points[c]
        n = (q - p).cross(r - p)
        tri = n.Length / 2
        if tri < 1e-12:
            continue
        if n.z / n.Length < limit and max(p.z, q.z, r.z) > zmin + 1e-3:
            area += tri
    return area
