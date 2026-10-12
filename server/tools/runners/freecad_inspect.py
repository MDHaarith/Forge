"""Measures parts headless: mass properties, holes, 3D-print overhangs, and interference between parts.

Forge launches: freecadcmd freecad_inspect.py with FORGE_PROJECT and FORGE_ARGS (JSON):
  models      project-relative .step/.stp/.brep/.FCStd files
  material    a name from forge_cad.MATERIALS, for mass (optional)
  overhang    maximum unsupported angle from vertical for the print check, degrees (default 45)
Each file's separate solids count as parts, so an assembly in one file is checked too.
"""

import itertools
import json
import os
import sys
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import forge_cad as forge  # noqa: E402


def done(code, line):
    print(line, flush=True)
    sys.stdout.flush()
    os._exit(code)


def r(x, n=3):
    return round(float(x), n)


def main():
    os.chdir(os.environ["FORGE_PROJECT"])
    args = json.loads(os.environ["FORGE_ARGS"])
    material = args.get("material")
    rho = forge.density(material) if material else None
    angle = float(args.get("overhang", 45))

    parts = []  # (label, solid) for the interference check
    reports = []
    for path in args["models"]:
        shape = forge.load(path)
        solids = shape.Solids
        box = shape.BoundBox
        volume = sum(s.Volume for s in solids)
        com = None
        if solids and volume > 0:
            c = solids[0].CenterOfMass * 0
            for s in solids:
                c += s.CenterOfMass * s.Volume
            c = c * (1 / volume)
            com = [r(c.x), r(c.y), r(c.z)]
        report = {
            "file": path,
            "valid": shape.isValid(),
            "solids": len(solids),
            "faces": len(shape.Faces),
            "volume_mm3": r(volume),
            "area_mm2": r(shape.Area),
            "size_mm": [r(box.XLength), r(box.YLength), r(box.ZLength)],
            "min_mm": [r(box.XMin), r(box.YMin), r(box.ZMin)],
            "center_of_mass_mm": com,
            "holes": forge.holes_in(shape),
        }
        if rho:
            report["mass_g"] = r(volume / 1000 * rho, 2)
        if solids:
            over = forge.overhangs(shape, angle)
            report["overhang_mm2"] = r(over, 1)
            report["overhang_percent"] = r(100 * over / shape.Area if shape.Area else 0, 1)
            # Footprint on the bed: faces at the lowest Z that face down.
            bed = sum(f.Area for f in shape.Faces if f.BoundBox.ZLength < 1e-6 and abs(f.BoundBox.ZMin - box.ZMin) < 1e-6)
            report["bed_contact_mm2"] = r(bed, 1)
        reports.append(report)
        name = os.path.basename(path)
        parts += [(f"{name}#{i + 1}" if len(solids) > 1 else name, s) for i, s in enumerate(solids)]

    pairs = []
    for (la, a), (lb, b) in itertools.combinations(parts, 2):
        near = a.BoundBox
        near.enlarge(5.0)
        if not near.intersect(b.BoundBox):
            continue  # more than 5 mm apart: nothing to report
        common = a.common(b)
        overlap = common.Volume if not common.isNull() else 0.0
        if overlap > 1e-6:
            pairs.append({"a": la, "b": lb, "interference_mm3": r(overlap)})
        else:
            pairs.append({"a": la, "b": lb, "clearance_mm": r(a.distToShape(b)[0])})

    done(0, "FORGE_RESULT " + json.dumps({"material": material, "density_g_cm3": rho, "overhang_angle": angle, "parts": reports, "pairs": pairs}))


try:
    main()
except BaseException:
    done(1, "FORGE_ERROR " + json.dumps("the inspection failed:\n" + traceback.format_exc(limit=8)))
