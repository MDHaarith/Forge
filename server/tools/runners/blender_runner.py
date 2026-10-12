"""Runs an agent-written bpy script inside headless Blender, then exports and renders the result.

Forge launches: blender --background --factory-startup --python-exit-code 1 --python blender_runner.py
and passes everything through FORGE_* environment variables. Lines starting with FORGE_RESULT
or FORGE_ERROR are read back by server/tools/blender.ts; everything else is Blender's own log.
"""

import json
import math
import os
import runpy
import sys
import traceback

import bpy
from mathutils import Vector

project = os.environ["FORGE_PROJECT"]
script = os.environ["FORGE_SCRIPT"]
name = os.environ["FORGE_NAME"]
formats = [f for f in os.environ.get("FORGE_FORMATS", "glb").split(",") if f]
render = os.environ.get("FORGE_RENDER") == "1"


def fail(message):
    print("FORGE_ERROR " + json.dumps(message), flush=True)
    sys.exit(1)


os.chdir(project)
bpy.ops.wm.read_factory_settings(use_empty=True)

try:
    runpy.run_path(script, run_name="__main__")
except SystemExit:
    raise
except BaseException:
    fail("the script raised an error:\n" + traceback.format_exc(limit=6))

scene = bpy.context.scene
solids = [o for o in scene.objects if o.type in {"MESH", "CURVE", "SURFACE", "META", "FONT"}]
if not solids:
    fail("the script created no visible objects")

os.makedirs("models", exist_ok=True)
files = []


def export(fmt, path):
    if fmt == "glb":
        bpy.ops.export_scene.gltf(filepath=path, export_format="GLB", export_apply=True)
    elif fmt == "stl":
        try:
            bpy.ops.wm.stl_export(filepath=path)  # Blender 4.1+
        except (AttributeError, RuntimeError):
            bpy.ops.export_mesh.stl(filepath=path, use_mesh_modifiers=True)
    elif fmt == "obj":
        bpy.ops.wm.obj_export(filepath=path)
    elif fmt == "blend":
        bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(path), copy=True)


for fmt in formats:
    path = f"models/{name}.{fmt}"
    try:
        export(fmt, path)
    except Exception as error:  # noqa: BLE001 - report every exporter failure to the agent
        hint = ""
        if "numpy" in str(error):
            hint = " (this Blender build lacks numpy, which the glTF exporter needs: install python3-numpy or use the official Blender download)"
        fail(f"exporting {fmt} failed: {error}{hint}")
    files.append(path)

# World-space bounds of everything that was built, after modifiers.
depsgraph = bpy.context.evaluated_depsgraph_get()
corners = [o.matrix_world @ Vector(c) for o in solids for c in o.bound_box]
lo = Vector((min(c.x for c in corners), min(c.y for c in corners), min(c.z for c in corners)))
hi = Vector((max(c.x for c in corners), max(c.y for c in corners), max(c.z for c in corners)))
size = hi - lo
center = (lo + hi) / 2
vertices = faces = 0
for o in solids:
    mesh = o.evaluated_get(depsgraph).to_mesh()
    vertices += len(mesh.vertices)
    faces += len(mesh.polygons)
    o.evaluated_get(depsgraph).to_mesh_clear()

if render:
    # Frame the subject, not the ground: flat objects such as floor planes are left out of the framing.
    subject = [o for o in solids if min(o.dimensions) > 1e-4 * max(max(o.dimensions), 1e-9)] or solids
    pts = [o.matrix_world @ Vector(c) for o in subject for c in o.bound_box]
    s_lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    s_hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    center = (s_lo + s_hi) / 2
    radius = max((s_hi - s_lo).length / 2, 0.01)
    if scene.camera is None:
        camera = bpy.data.objects.new("Forge camera", bpy.data.cameras.new("Forge camera"))
        scene.collection.objects.link(camera)
        direction = Vector((1.0, -1.2, 0.8)).normalized()
        # Back off until the subject's bounding sphere fits the narrower (vertical) field of view.
        half_fov = math.atan(math.tan(camera.data.angle_x / 2) * 720 / 960)
        camera.location = center + direction * (radius / math.sin(half_fov)) * 1.05
        camera.rotation_euler = (-direction).to_track_quat("-Z", "Y").to_euler()
        camera.data.clip_end = radius * 50
        scene.camera = camera
    if not any(o.type == "LIGHT" for o in scene.objects):
        for energy, angle in ((3.0, (math.radians(50), 0, math.radians(30))), (1.0, (math.radians(60), 0, math.radians(200)))):
            sun = bpy.data.objects.new("Forge light", bpy.data.lights.new("Forge light", "SUN"))
            sun.data.energy = energy
            sun.rotation_euler = angle
            scene.collection.objects.link(sun)
    if scene.world is None:
        scene.world = bpy.data.worlds.new("Forge world")
        scene.world.color = (0.05, 0.05, 0.06)
    # Cycles on the CPU renders on any machine, including headless servers with no GPU.
    if scene.render.engine != "CYCLES":
        scene.render.engine = "CYCLES"
        scene.cycles.samples = 48
    scene.cycles.device = "CPU"
    scene.render.resolution_x, scene.render.resolution_y = 960, 720
    scene.render.resolution_percentage = 100
    os.makedirs("renders", exist_ok=True)
    scene.render.filepath = os.path.abspath(f"renders/{name}.png")
    scene.render.image_settings.file_format = "PNG"
    try:
        bpy.ops.render.render(write_still=True)
    except RuntimeError as error:
        # Some Linux distribution builds ship Cycles without the OpenImageDenoise library.
        if "Denoise" not in str(error):
            fail(f"rendering failed: {error}")
        scene.cycles.use_denoising = False
        try:
            bpy.ops.render.render(write_still=True)
        except RuntimeError as again:
            fail(f"rendering failed: {again}")
    files.append(f"renders/{name}.png")

print(
    "FORGE_RESULT "
    + json.dumps(
        {
            "objects": [o.name for o in solids],
            "vertices": vertices,
            "faces": faces,
            "size": [round(v, 4) for v in size],
            "files": files,
        }
    ),
    flush=True,
)
