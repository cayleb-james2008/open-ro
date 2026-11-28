"""OpenRO mesh pipeline, run inside headless Blender (embedded by src/mesh.rs).

    blender -b --factory-startup --python mesh_lib.py -- spec.json out_dir

Build (structured parts and/or an agent-written bpy script) -> smart UV unwrap + island
pack -> Cycles bake of colour x ambient occlusion (warm shadow tint) -> OBJ (Y-up, 1 unit =
1 stud) + PNG, a separate `<name>_glow` mesh for glow parts -> manifest.json -> 2 previews.

Structured parts are authored Y-up like Roblox (x right, y up, z toward the viewer), in studs
and degrees. A `script` runs in Blender's own Z-up space; the export converts to Y-up.
"""
import json
import math
import os
import signal
import struct
import sys

import bmesh
import bpy
import numpy as np
from mathutils import Euler, Matrix, Vector

MAX_TRIS = 20000
SCRIPT_TIMEOUT = 90
S_YUP = Matrix.Rotation(math.radians(90), 4, "X")  # Y-up studs -> Blender Z-up
A_AXIS = Matrix.Rotation(math.radians(-90), 4, "X")  # primitive Z axis -> Y axis


class Fail(Exception):
    """A problem to report to the user in plain words."""


def r3(v):
    return round(float(v), 3) + 0.0


def r4(v):
    return round(float(v), 4) + 0.0


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def parse_hex(h):
    h = (h or "#b8b8c0").strip().lstrip("#")
    if len(h) == 3:
        h = "".join(ch * 2 for ch in h)
    if len(h) != 6:
        raise Fail(f"colour '{h}' is not a valid hex colour like #ff8800")
    try:
        return tuple(int(h[i:i + 2], 16) / 255.0 for i in (0, 2, 4))
    except ValueError:
        raise Fail(f"colour '#{h}' is not a valid hex colour like #ff8800")


def ensure_nodes(datablock):
    """Return node_tree without triggering Blender 5.2+ use_nodes deprecation warnings."""
    nt = getattr(datablock, "node_tree", None)
    if nt is None:
        datablock.use_nodes = True
        nt = datablock.node_tree
    return nt


def ensure_principled(mat):
    nt = ensure_nodes(mat)
    bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if bsdf is None:
        bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
        out = next((n for n in nt.nodes if n.type == "OUTPUT_MATERIAL"), None)
        if out is None:
            out = nt.nodes.new("ShaderNodeOutputMaterial")
        if not out.inputs["Surface"].is_linked:
            nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    return nt, bsdf


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.unit_settings.system = "NONE"


def material_for(hexcol):
    key = "c_" + (hexcol or "#b8b8c0").strip().lstrip("#").lower()
    if key in bpy.data.materials:
        return bpy.data.materials[key]
    r, g, b = (srgb_to_linear(v) for v in parse_hex(hexcol))
    m = bpy.data.materials.new(key)
    _, bsdf = ensure_principled(m)
    bsdf.inputs["Base Color"].default_value = (r, g, b, 1.0)
    return m


def make_stone_material(name, base_hex, crevice_hex="#151b32", noise_scale=5.5, roughness=0.78):
    """Procedural Cycles stone/temple material with Voronoi masonry cracks & Noise grain."""
    if name in bpy.data.materials:
        return bpy.data.materials[name]
    r1, g1, b1 = (srgb_to_linear(v) for v in parse_hex(base_hex))
    r0, g0, b0 = (srgb_to_linear(v) for v in parse_hex(crevice_hex))
    m = bpy.data.materials.new(name)
    nt, bsdf = ensure_principled(m)
    bsdf.inputs["Roughness"].default_value = roughness
    tex = nt.nodes.new("ShaderNodeTexCoord")
    noise = nt.nodes.new("ShaderNodeTexNoise")
    noise.inputs["Scale"].default_value = float(noise_scale)
    noise.inputs["Detail"].default_value = 6.0
    noise.inputs["Roughness"].default_value = 0.65
    vor = nt.nodes.new("ShaderNodeTexVoronoi")
    vor.feature = "DISTANCE_TO_EDGE"
    vor.inputs["Scale"].default_value = float(noise_scale) * 0.45
    mix_f = nt.nodes.new("ShaderNodeMath")
    mix_f.operation = "MULTIPLY"
    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].position = 0.08
    ramp.color_ramp.elements[0].color = (r0, g0, b0, 1.0)
    ramp.color_ramp.elements[1].position = 0.62
    ramp.color_ramp.elements[1].color = (r1, g1, b1, 1.0)
    nt.links.new(tex.outputs["Object"], noise.inputs["Vector"])
    nt.links.new(tex.outputs["Object"], vor.inputs["Vector"])
    nt.links.new(noise.outputs["Fac"], mix_f.inputs[0])
    nt.links.new(vor.outputs["Distance"], mix_f.inputs[1])
    nt.links.new(mix_f.outputs["Value"], ramp.inputs["Fac"])
    nt.links.new(ramp.outputs["Color"], bsdf.inputs["Base Color"])
    return m


def make_metallic_material(name, base_hex, dark_hex="#181e2c", roughness=0.35):
    if name in bpy.data.materials:
        return bpy.data.materials[name]
    m = make_stone_material(name, base_hex, dark_hex, noise_scale=9.0, roughness=roughness)
    _, bsdf = ensure_principled(m)
    if "Metallic" in bsdf.inputs:
        bsdf.inputs["Metallic"].default_value = 0.45
    return m


def make_glow_material(name, hex_col, strength=3.0):
    if name in bpy.data.materials:
        return bpy.data.materials[name]
    r, g, b = (srgb_to_linear(v) for v in parse_hex(hex_col))
    m = bpy.data.materials.new(name)
    _, bsdf = ensure_principled(m)
    bsdf.inputs["Base Color"].default_value = (r, g, b, 1.0)
    if "Emission Color" in bsdf.inputs:
        bsdf.inputs["Emission Color"].default_value = (r, g, b, 1.0)
        bsdf.inputs["Emission Strength"].default_value = float(strength)
    return m


def bevel_obj(ob, width=0.06, segments=2):
    mod = ob.modifiers.new("Bevel", "BEVEL")
    mod.width = float(width)
    mod.segments = int(segments)
    mod.limit_method = "ANGLE"
    mod.angle_limit = math.radians(32)
    mod.use_clamp_overlap = True
    return mod


# ---------------------------------------------------------------- structured parts
class Builder:
    """Accumulates parts into one solid mesh and one glow mesh (bmesh, no bpy.ops)."""

    def __init__(self, name):
        self.name = name
        self.groups = {False: (bmesh.new(), []), True: (bmesh.new(), [])}  # glow -> (bm, colours)

    def add_part(self, shape, size=(1, 1, 1), pos=(0, 0, 0), rot=(0, 0, 0), color="#b8b8c0",
                 glow=False, segments=None, tube=None):
        bm, cols = self.groups[bool(glow)]
        if color not in cols:
            cols.append(color)
        mi = cols.index(color)
        sx, sy, sz = (float(v) for v in size)
        before = set(bm.verts)
        seg = int(segments) if segments else None
        smooth = True
        local = Matrix.Identity(4)
        if shape == "box":
            bmesh.ops.create_cube(bm, size=1.0)
            local = Matrix.Diagonal(Vector((sx, sy, sz, 1)))
            smooth = False
        elif shape in ("cylinder", "cone"):
            bmesh.ops.create_cone(
                bm,
                cap_ends=True,
                segments=seg or 16,
                radius1=0.5,
                radius2=0.5 if shape == "cylinder" else 0.0,
                depth=1.0,
            )
            local = Matrix.Diagonal(Vector((sx, sy, sz, 1))) @ A_AXIS
        elif shape == "sphere":
            n = seg or 16
            bmesh.ops.create_uvsphere(bm, u_segments=n, v_segments=max(3, n // 2 + 1), radius=0.5)
            local = Matrix.Diagonal(Vector((sx, sy, sz, 1))) @ A_AXIS
        elif shape == "wedge":
            c = [
                (-0.5, -0.5, -0.5),
                (0.5, -0.5, -0.5),
                (0.5, -0.5, 0.5),
                (-0.5, -0.5, 0.5),
                (-0.5, 0.5, 0.5),
                (0.5, 0.5, 0.5),
            ]
            v = [bm.verts.new(p) for p in c]
            for idx in ((0, 1, 2, 3), (3, 2, 5, 4), (0, 3, 4), (1, 5, 2), (0, 4, 5, 1)):
                bm.faces.new([v[i] for i in idx])
            local = Matrix.Diagonal(Vector((sx, sy, sz, 1)))
            smooth = False
        elif shape == "torus":
            tube_d = float(tube) if tube is not None else sy
            if tube_d <= 0.0 or min(sx, sz) <= tube_d:
                raise Fail(
                    f"torus size [{sx}, {sy}, {sz}] is invalid: outer X and Z diameters must be "
                    "larger than tube thickness Y (use rot [90, 0, 0] to stand a ring upright)"
                )
            small_r = tube_d / 2.0
            big_rx = sx / 2.0 - small_r
            big_rz = sz / 2.0 - small_r
            n = seg or 24
            rn = max(6, n // 3)
            rings = []
            for i in range(n):
                a = 2.0 * math.pi * i / n
                ca, sa = math.cos(a), math.sin(a)
                ring = []
                for j in range(rn):
                    t = 2.0 * math.pi * j / rn
                    ct, st = math.cos(t), math.sin(t)
                    ring.append(bm.verts.new((
                        (big_rx + small_r * ct) * ca,
                        (big_rz + small_r * ct) * sa,
                        small_r * st,
                    )))
                rings.append(ring)
            for i in range(n):
                a_, b_ = rings[i], rings[(i + 1) % n]
                for j in range(rn):
                    bm.faces.new((a_[j], a_[(j + 1) % rn], b_[(j + 1) % rn], b_[j]))
            local = A_AXIS
        else:
            raise Fail(
                f"unknown shape '{shape}' (expected box, cylinder, cone, sphere, wedge, or torus)"
            )
        rx, ry, rz = (math.radians(float(a)) for a in rot)
        m = (
            S_YUP
            @ Matrix.Translation(Vector(pos))
            @ Euler((rx, ry, rz), "XYZ").to_matrix().to_4x4()
            @ local
        )
        new_verts = [x for x in bm.verts if x not in before]
        bmesh.ops.transform(bm, matrix=m, verts=new_verts)
        for f in {f for x in new_verts for f in x.link_faces}:
            f.material_index = mi
            f.smooth = smooth

    def make_objects(self, bevel):
        out = []
        for glow, (bm, cols) in self.groups.items():
            if not bm.verts:
                bm.free()
                continue
            bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
            me = bpy.data.meshes.new(self.name + ("_glow" if glow else ""))
            bm.to_mesh(me)
            bm.free()
            for c in cols:
                me.materials.append(material_for(c))
            ob = bpy.data.objects.new(me.name, me)
            bpy.context.scene.collection.objects.link(ob)
            if bevel > 0.0:
                md = ob.modifiers.new("bevel", "BEVEL")
                md.width = bevel
                md.segments = 1
                md.use_clamp_overlap = True
                md.limit_method = "ANGLE"
                md.angle_limit = math.radians(35)
            out.append(ob)
        return out


# ---------------------------------------------------------------- collect / join
def tri_count(me):
    return sum(max(0, len(p.vertices) - 2) for p in me.polygons)


def collect(name):
    """Evaluate every mesh object (modifiers applied, world transform baked), enforce tri limit
    early, then join into one solid object and one glow object. Empties named anchor* become anchors."""
    if bpy.context.mode != "OBJECT":
        try:
            bpy.ops.object.mode_set(mode="OBJECT")
        except Exception:
            pass
    dg = bpy.context.evaluated_depsgraph_get()
    solid, glow, anchors = [], [], []
    running_tris = 0
    for ob in list(bpy.context.scene.objects):
        if ob.type == "EMPTY" and ob.name.lower().startswith("anchor"):
            p = ob.matrix_world.translation
            aname = ob.name.lower()
            if aname.startswith("anchor_"):
                aname = aname[len("anchor_"):]
            anchors.append({"name": aname or "anchor", "pos": [r4(p.x), r4(p.z), r4(-p.y)]})
            continue
        if ob.type != "MESH" or ob.hide_render:
            continue
        me = bpy.data.meshes.new_from_object(ob.evaluated_get(dg))
        me.transform(ob.matrix_world)
        if not me.polygons:
            bpy.data.meshes.remove(me)
            continue
        running_tris += tri_count(me)
        if running_tris > MAX_TRIS:
            raise Fail(
                f"this mesh has {running_tris} triangles (over the {MAX_TRIS} triangle limit Roblox allows). "
                "Use fewer or simpler parts (lower segments) and try again."
            )
        if not me.materials:
            me.materials.append(material_for("#b8b8c0"))
        for mat in me.materials:
            if mat is None:
                raise Fail("an object has an empty material slot; assign a material to every slot")
            ensure_nodes(mat)
        nob = bpy.data.objects.new(ob.name, me)
        lowered = ob.name.lower()
        is_glow = lowered.endswith("glow") or lowered.startswith("glow") or bool(ob.get("glow"))
        (glow if is_glow else solid).append(nob)
    for ob in list(bpy.context.scene.objects):
        bpy.data.objects.remove(ob, do_unlink=True)

    def join(obs, nm):
        if not obs:
            return None
        for o in obs:
            bpy.context.scene.collection.objects.link(o)
        if len(obs) > 1:
            with bpy.context.temp_override(
                active_object=obs[0],
                object=obs[0],
                selected_objects=obs,
                selected_editable_objects=obs,
            ):
                bpy.ops.object.join()
        obs[0].name = nm
        obs[0].data.name = nm
        return obs[0]

    return join(solid, name), join(glow, name + "_glow"), anchors


# ---------------------------------------------------------------- unwrap + bake
def select_only(ob):
    for o in bpy.context.scene.objects:
        o.select_set(o is ob)
    bpy.context.view_layer.objects.active = ob


def unwrap(ob):
    select_only(ob)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(62), island_margin=0.02, scale_to_bounds=False)
    bpy.ops.uv.average_islands_scale()
    bpy.ops.uv.pack_islands(rotate=True, margin=0.004)
    bpy.ops.object.mode_set(mode="OBJECT")


def bake_target(ob, img):
    for mat in ob.data.materials:
        nt = ensure_nodes(mat)
        n = nt.nodes.get("BAKE") or nt.nodes.new("ShaderNodeTexImage")
        n.name = "BAKE"
        n.image = img
        nt.nodes.active = n
        n.select = True


def bake(ob, path, size, ao_strength, samples, with_ao=True):
    sc = bpy.context.scene
    sc.cycles.samples = samples
    sc.cycles.use_denoising = False
    select_only(ob)
    colour = bpy.data.images.new(ob.name + "_c", size, size, alpha=False)
    bake_target(ob, colour)
    bpy.ops.object.bake(type="DIFFUSE", pass_filter={"COLOR"}, margin=8, margin_type="EXTEND")
    c = np.array(colour.pixels[:], dtype=np.float32).reshape(size, size, 4)
    if with_ao and ao_strength > 0.0:
        ao = bpy.data.images.new(ob.name + "_ao", size, size, alpha=False)
        bake_target(ob, ao)
        sc.cycles.samples = max(samples, 32)
        bpy.ops.object.bake(type="AO", margin=8, margin_type="EXTEND")
        a = np.array(ao.pixels[:], dtype=np.float32).reshape(size, size, 4)
        shade = 1.0 - ao_strength * (1.0 - a[..., :1])
        # Stylized warm shadow tint instead of flat grey
        tint = np.array([1.0, 0.82, 0.70], dtype=np.float32)
        shadow = 1.0 - shade
        c[..., :3] = np.clip(c[..., :3] * (shade + shadow * tint * 0.55), 0.0, 1.0)
    c[..., 3] = 1.0
    out = bpy.data.images.new(ob.name + "_final", size, size, alpha=False)
    out.pixels = c.ravel().tolist()
    out.filepath_raw = path
    out.file_format = "PNG"
    out.save()
    rgb = c[..., :3]
    return {
        "file": os.path.basename(path),
        "size": size,
        "mean": [round(float(v), 4) for v in rgb.reshape(-1, 3).mean(axis=0)],
        "std": round(float(rgb.std()), 5),
    }


# ---------------------------------------------------------------- export + preview
def export_obj(ob, path):
    select_only(ob)
    bpy.ops.wm.obj_export(
        filepath=path,
        export_selected_objects=True,
        forward_axis="NEGATIVE_Z",
        up_axis="Y",
        export_materials=False,
        export_uv=True,
        export_normals=True,
        export_triangulated_mesh=True,
        apply_modifiers=True,
    )


def export_roblox_mesh_v2(ob, path):
    """Export Y-up triangulated mesh in native Roblox binary FileMesh version 2.00 format (36B vertices)."""
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(ob.evaluated_get(dg))
    me.transform(ob.matrix_world)
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    bm.to_mesh(me)
    bm.free()
    me.calc_loop_triangles()
    uv_layer = me.uv_layers.active.data if me.uv_layers.active else None

    verts_blob = bytearray()
    faces_blob = bytearray()
    vert_map = {}
    vert_count = 0
    face_count = 0

    for tri in me.loop_triangles:
        f_idx = []
        nrm_f = tri.normal
        for k in range(3):
            li = tri.loops[k]
            vi = tri.vertices[k]
            co = me.vertices[vi].co
            vno = me.vertices[vi].normal if tri.use_smooth else nrm_f
            # Y-up conversion matching export_obj (-Z forward, +Y up): (x, z, -y)
            px, py, pz = float(co.x), float(co.z), float(-co.y)
            nx, ny, nz = float(vno.x), float(vno.z), float(-vno.y)
            nlen = math.sqrt(nx * nx + ny * ny + nz * nz) or 1.0
            nx, ny, nz = nx / nlen, ny / nlen, nz / nlen
            if uv_layer is not None:
                uv = uv_layer[li].uv
                tu, tv = float(uv.x), float(1.0 - uv.y)
            else:
                tu, tv = 0.0, 0.0
            key = (
                round(px, 5), round(py, 5), round(pz, 5),
                round(nx, 4), round(ny, 4), round(nz, 4),
                round(tu, 5), round(tv, 5),
            )
            idx = vert_map.get(key)
            if idx is None:
                idx = vert_count
                vert_map[key] = idx
                vert_count += 1
                verts_blob.extend(struct.pack(
                    "<8f4b",
                    px, py, pz,
                    nx, ny, nz,
                    tu, tv,
                    127, 0, 0, 127,
                ))
            f_idx.append(idx)
        # Swap winding to keep front-facing outward normals under Y-up coordinate mirror
        faces_blob.extend(struct.pack("<3I", f_idx[0], f_idx[1], f_idx[2]))
        face_count += 1

    bpy.data.meshes.remove(me)
    header = b"version 2.00\n" + struct.pack("<HBBII", 12, 36, 12, vert_count, face_count)
    with open(path, "wb") as fh:
        fh.write(header)
        fh.write(verts_blob)
        fh.write(faces_blob)


def export_glb(ob, path):
    try:
        select_only(ob)
        bpy.ops.export_scene.gltf(
            filepath=path,
            export_format="GLB",
            use_selection=True,
            export_yup=True,
            export_materials="NONE",
        )
        return os.path.isfile(path)
    except Exception:
        return False


def bounds(obs):
    pts = np.concatenate([
        np.array([(o.matrix_world @ Vector(c))[:] for c in o.bound_box]) for o in obs
    ])
    return Vector(pts.min(axis=0)), Vector(pts.max(axis=0))


def preview_material(ob, png, emit):
    ob.data.materials.clear()
    m = bpy.data.materials.new("preview_" + ob.name)
    nt, bsdf = ensure_principled(m)
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = bpy.data.images.load(png)
    nt.links.new(t.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.7
    if emit:
        nt.links.new(t.outputs["Color"], bsdf.inputs["Emission Color"])
        bsdf.inputs["Emission Strength"].default_value = 2.5
    ob.data.materials.append(m)


def render_previews(obs, base_png, res):
    sc = bpy.context.scene
    sc.render.resolution_x = sc.render.resolution_y = res
    sc.cycles.samples = 24
    sc.cycles.use_denoising = False
    w = bpy.data.worlds.new("w")
    wnt = ensure_nodes(w)
    bg = next((n for n in wnt.nodes if n.type == "BACKGROUND"), None)
    if bg is None:
        bg = wnt.nodes.new("ShaderNodeBackground")
        wout = next((n for n in wnt.nodes if n.type == "OUTPUT_WORLD"), None)
        if wout is None:
            wout = wnt.nodes.new("ShaderNodeOutputWorld")
        wnt.links.new(bg.outputs["Background"], wout.inputs["Surface"])
    bg.inputs[0].default_value = (0.62, 0.80, 1.0, 1.0)
    bg.inputs[1].default_value = 1.0
    sc.world = w
    sc.view_settings.view_transform = "Standard"
    lo, hi = bounds(obs)
    centre = (lo + hi) / 2.0
    size = max((hi - lo).length, 1.0)
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sun.data.energy = 3.4
    sun.rotation_euler = (math.radians(50), math.radians(10), math.radians(-30))
    sc.collection.objects.link(sun)
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    cam.data.lens = 50
    sc.collection.objects.link(cam)
    sc.camera = cam
    gm = bpy.data.materials.new("ground")
    _, gbsdf = ensure_principled(gm)
    gbsdf.inputs["Base Color"].default_value = (0.35, 0.62, 0.34, 1.0)
    gme = bpy.data.meshes.new("ground")
    g = size * 4.0
    gme.from_pydata([(-g, -g, 0), (g, -g, 0), (g, g, 0), (-g, g, 0)], [], [(0, 1, 2, 3)])
    gme.materials.append(gm)
    gob = bpy.data.objects.new("ground", gme)
    gob.location = (centre.x, centre.y, lo.z - 0.01)
    sc.collection.objects.link(gob)
    paths = []
    for k, (elev, yaw) in enumerate(((28, 30), (28, 150))):
        d = size * 1.45
        cam.location = centre + Vector((
            d * math.cos(math.radians(elev)) * math.sin(math.radians(yaw)),
            -d * math.cos(math.radians(elev)) * math.cos(math.radians(yaw)),
            d * math.sin(math.radians(elev)),
        ))
        cam.rotation_euler = (centre - cam.location).to_track_quat("-Z", "Y").to_euler()
        p = base_png.replace(".png", f"_{k}.png")
        sc.render.filepath = p
        bpy.ops.render.render(write_still=True)
        paths.append(os.path.basename(p))
    return paths


# ---------------------------------------------------------------- main
class Timeout(Exception):
    pass


def run_user_script(src, ns, timeout_secs):
    if hasattr(signal, "SIGALRM"):
        def on_alarm(_s, _f):
            raise Timeout()
        signal.signal(signal.SIGALRM, on_alarm)
        signal.alarm(int(timeout_secs))
    try:
        exec(compile(src, "<openro-script>", "exec"), ns)
    except Timeout:
        raise Fail(f"your script ran longer than {int(timeout_secs)} seconds and was stopped")
    except Fail:
        raise
    except Exception as e:
        raise Fail(f"your script failed: {type(e).__name__}: {e}")
    finally:
        if hasattr(signal, "SIGALRM"):
            signal.alarm(0)


def main(spec, out_dir):
    name = spec["slug"]
    size = int(spec.get("texture_size", 1024))
    timeout_secs = int(spec.get("script_timeout_secs", SCRIPT_TIMEOUT))
    reset()
    builder = Builder(name)
    ns = {
        "bpy": bpy,
        "bmesh": bmesh,
        "math": math,
        "np": np,
        "Vector": Vector,
        "Matrix": Matrix,
        "Euler": Euler,
        "add_part": builder.add_part,
        "material_for": material_for,
        "make_stone_material": make_stone_material,
        "make_metallic_material": make_metallic_material,
        "make_glow_material": make_glow_material,
        "bevel_obj": bevel_obj,
        "__name__": "openro_script",
    }
    for p in spec.get("parts") or []:
        builder.add_part(
            p["shape"],
            p["size"],
            p["pos"],
            p["rot"],
            p["color"],
            p.get("glow", False),
            p.get("segments"),
            p.get("tube"),
        )
    if spec.get("script"):
        run_user_script(spec["script"], ns, timeout_secs)
    builder.make_objects(float(spec.get("bevel", 0.03)))
    solid, glow, anchors = collect(name)
    if solid is None and glow is None:
        raise Fail("nothing was built: add at least one part, or make your script create a mesh object")
    tris = tri_count(solid.data) if solid else 0
    glow_tris = tri_count(glow.data) if glow else 0
    total_tris = tris + glow_tris
    if total_tris > MAX_TRIS:
        raise Fail(
            f"this mesh has {total_tris} triangles, over the {MAX_TRIS} triangle limit Roblox allows. "
            "Use fewer or simpler parts (lower segments) and try again."
        )
    obs = [o for o in (solid, glow) if o]
    for o in obs:
        unwrap(o)
    ao_strength = float(spec.get("ao_strength", 0.75))
    tex = {}
    has_glb = False
    has_glow_glb = False
    if solid:
        tex["solid"] = bake(solid, os.path.join(out_dir, name + ".png"), size, ao_strength, 24)
        export_obj(solid, os.path.join(out_dir, name + ".obj"))
        export_roblox_mesh_v2(solid, os.path.join(out_dir, name + ".mesh"))
        has_glb = export_glb(solid, os.path.join(out_dir, name + ".glb"))
    if glow:
        tex["glow"] = bake(
            glow,
            os.path.join(out_dir, name + "_glow.png"),
            min(size, 512),
            0.0,
            16,
            with_ao=False,
        )
        export_obj(glow, os.path.join(out_dir, name + "_glow.obj"))
        export_roblox_mesh_v2(glow, os.path.join(out_dir, name + "_glow.mesh"))
        has_glow_glb = export_glb(glow, os.path.join(out_dir, name + "_glow.glb"))
    lo, hi = bounds(obs)
    previews = []
    if spec.get("preview", True):
        if solid:
            preview_material(solid, os.path.join(out_dir, name + ".png"), False)
        if glow:
            preview_material(glow, os.path.join(out_dir, name + "_glow.png"), True)
        previews = render_previews(
            obs,
            os.path.join(out_dir, name + "_preview.png"),
            int(spec.get("preview_size", 480)),
        )
    anchors = list(spec.get("anchors") or []) + anchors
    names = {a["name"] for a in anchors}
    cx = (lo.x + hi.x) / 2.0
    cz = -(lo.y + hi.y) / 2.0
    for nm, pos in (
        ("base-centre", [cx, lo.z, cz]),
        ("top-centre", [cx, hi.z, cz]),
        ("centre", [cx, (lo.z + hi.z) / 2.0, cz]),
    ):
        if nm not in names:
            anchors.append({"name": nm, "pos": [r4(v) for v in pos]})
    manifest = {
        "name": name,
        "units": "studs",
        "up_axis": "Y",
        "mesh": name + ".obj" if solid else None,
        "roblox_mesh": name + ".mesh" if solid else None,
        "glb": name + ".glb" if (solid and has_glb) else None,
        "texture": name + ".png" if solid else None,
        "glow_mesh": name + "_glow.obj" if glow else None,
        "glow_roblox_mesh": name + "_glow.mesh" if glow else None,
        "glow_glb": name + "_glow.glb" if (glow and has_glow_glb) else None,
        "glow_texture": name + "_glow.png" if glow else None,
        "tris": total_tris,
        "solid_tris": tris,
        "glow_tris": glow_tris,
        "size": [r3(hi.x - lo.x), r3(hi.z - lo.z), r3(hi.y - lo.y)],
        "bbox": {
            "min": [r3(lo.x), r3(lo.z), r3(-hi.y)],
            "max": [r3(hi.x), r3(hi.z), r3(-lo.y)],
        },
        "anchors": anchors,
        "texture_stats": tex,
        "texture_size": size,
        "previews": previews,
    }
    with open(os.path.join(out_dir, name + ".manifest.json"), "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, indent=2)
    print("OPENRO_MESH_OK", manifest["tris"], manifest["size"], flush=True)


if "--" in sys.argv:
    argv = sys.argv[sys.argv.index("--") + 1:]
    with open(argv[0], encoding="utf-8") as fh:
        _spec = json.load(fh)
    try:
        main(_spec, argv[1])
    except Fail as e:
        with open(os.path.join(argv[1], "error.txt"), "w", encoding="utf-8") as fh:
            fh.write(str(e))
        print("OPENRO_MESH_ERROR", e, flush=True)
        sys.exit(3)
