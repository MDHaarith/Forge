"""Runs an agent-written FreeCAD script headless and exports the resulting solid.

Forge launches: freecadcmd freecad_runner.py with FORGE_* environment variables.
The script gets App/FreeCAD, Part, Vector, `forge` (engineering helpers, forge_cad.py) and a
fresh document `doc`. It either assigns `result = <Part.Shape>`, or adds objects to `doc`
(Part features or PartDesign bodies); then every top-level solid is exported.
Lines starting with FORGE_RESULT or FORGE_ERROR are read back by server/tools/freecad.ts.
"""

import json
import os
import runpy
import sys
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import FreeCAD as App  # noqa: E402
import Part  # noqa: E402

import forge_cad  # noqa: E402


def done(code, line):
    print(line, flush=True)
    sys.stdout.flush()
    # FreeCADCmd swallows SystemExit and can fall through to its console, so leave directly.
    os._exit(code)


def fail(message):
    done(1, "FORGE_ERROR " + json.dumps(message))


try:
    project = os.environ["FORGE_PROJECT"]
    script = os.environ["FORGE_SCRIPT"]
    name = os.environ["FORGE_NAME"]
    formats = [f for f in os.environ.get("FORGE_FORMATS", "step,stl,fcstd").split(",") if f]
    os.chdir(project)

    doc = App.newDocument(name.replace("-", "_"))
    try:
        scope = runpy.run_path(script, init_globals={"App": App, "FreeCAD": App, "Part": Part, "Vector": App.Vector, "forge": forge_cad, "doc": doc}, run_name="__main__")
    except BaseException:
        fail("the script raised an error:\n" + traceback.format_exc(limit=6))
    doc.recompute()

    shape = scope.get("result")
    if shape is None:
        # Top-level objects: those no other object consumes (e.g. a Cut's inputs are skipped).
        tops = [o for o in doc.Objects if hasattr(o, "Shape") and not o.InList and not o.Shape.isNull()]
        # Prefer solids, so leftover sketches or construction geometry are not exported.
        tops = [o for o in tops if o.Shape.Solids] or tops
        if not tops:
            fail("the script produced no shape: set `result = <shape>` or add objects to `doc`")
        shape = tops[0].Shape if len(tops) == 1 else Part.makeCompound([o.Shape for o in tops])
    elif not isinstance(shape, Part.Shape):
        fail("`result` must be a Part.Shape")
    # A shape handed over as `result` still goes into the saved document.
    if not any(hasattr(o, "Shape") for o in doc.Objects):
        doc.addObject("Part::Feature", "Result").Shape = shape
        doc.recompute()

    os.makedirs("models", exist_ok=True)
    files = []
    for fmt in formats:
        path = f"models/{name}.{fmt.lower() if fmt != 'fcstd' else 'FCStd'}"
        if fmt == "step":
            shape.exportStep(path)
        elif fmt == "stl":
            import MeshPart

            mesh = MeshPart.meshFromShape(Shape=shape, LinearDeflection=0.05, AngularDeflection=0.25, Relative=False)
            mesh.write(path)
        elif fmt == "fcstd":
            # Overwriting would leave a timestamped .FCBak beside it; git already keeps the history.
            if os.path.exists(path):
                os.remove(path)
            doc.saveAs(os.path.abspath(path))
        files.append(path)

    box = shape.BoundBox
    done(
        0,
        "FORGE_RESULT "
        + json.dumps(
            {
                "valid": shape.isValid(),
                "solids": len(shape.Solids),
                "volume_mm3": round(shape.Volume, 3),
                "area_mm2": round(shape.Area, 3),
                "size_mm": [round(box.XLength, 4), round(box.YLength, 4), round(box.ZLength, 4)],
                "min_mm": [round(box.XMin, 4), round(box.YMin, 4), round(box.ZMin, 4)],
                "files": files,
            }
        ),
    )
except BaseException:
    fail("Forge's FreeCAD runner failed:\n" + traceback.format_exc(limit=6))
