/**
 * Blender 3D Asset Pipeline Script Templates
 * Self-contained Python 3 scripts compatible with Blender 3.3+ bpy engine.
 * Embedded directly in TypeScript to ensure runtime reliability in packaged Electron builds.
 */

export const OPENRO_3D_SENTINEL_START = '###OPENRO_3D_OUTPUT_START###';
export const OPENRO_3D_SENTINEL_END = '###OPENRO_3D_OUTPUT_END###';

export const OPENRO_3D_PIPELINE_SCRIPT = `"""
Open-Ro Blender 3D Asset Pipeline & Headless Task Engine
Self-contained Python 3 script compatible with Blender 3.3+ bpy API.
Executes headless 3D asset processing tasks with sandboxing and structured JSON output.
"""

import sys
import os
import json
import time
import math
import traceback
import shutil

SENTINEL_START = "${OPENRO_3D_SENTINEL_START}"
SENTINEL_END = "${OPENRO_3D_SENTINEL_END}"

def validate_sandboxed_path(target_path, allowed_roots):
    """Verifies that target_path does not escape allowed roots (defense against path traversal)."""
    if not target_path or "\\0" in target_path:
        raise ValueError(f"Invalid path containing null bytes or empty: {target_path!r}")
    abs_target = os.path.abspath(target_path)
    real_target = os.path.realpath(abs_target)
    norm_target = os.path.normcase(real_target)
    if not allowed_roots:
        return real_target
    for root in allowed_roots:
        if not root:
            continue
        real_root = os.path.realpath(os.path.abspath(root))
        norm_root = os.path.normcase(real_root)
        if norm_target == norm_root or norm_target.startswith(norm_root + os.sep):
            return real_target
    raise PermissionError(
        f"Path traversal rejected: {target_path} resolves to {real_target}, "
        f"outside authorized roots: {allowed_roots}"
    )

def clear_blender_scene(bpy):
    """Removes all objects, meshes, materials, and textures from the active Blender data."""
    if hasattr(bpy.ops.wm, "read_factory_settings"):
        try:
            bpy.ops.wm.read_factory_settings(use_empty=True)
            return
        except Exception:
            pass
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for mesh in list(bpy.data.meshes):
        bpy.data.meshes.remove(mesh, do_unlink=True)
    for mat in list(bpy.data.materials):
        bpy.data.materials.remove(mat, do_unlink=True)

def import_mesh_file(bpy, file_path):
    """Imports 3D model according to its file extension using version-appropriate bpy operators."""
    ext = os.path.splitext(file_path)[1].lower()
    if ext == ".obj":
        if hasattr(bpy.ops.wm, "obj_import"):
            bpy.ops.wm.obj_import(filepath=file_path)
        elif hasattr(bpy.ops.import_scene, "obj"):
            bpy.ops.import_scene.obj(filepath=file_path)
        else:
            raise RuntimeError("No OBJ importer available in this Blender installation.")
    elif ext == ".fbx":
        if hasattr(bpy.ops.import_scene, "fbx"):
            bpy.ops.import_scene.fbx(filepath=file_path)
        else:
            raise RuntimeError("No FBX importer available in this Blender installation.")
    elif ext in [".gltf", ".glb"]:
        if hasattr(bpy.ops.import_scene, "gltf"):
            bpy.ops.import_scene.gltf(filepath=file_path)
        else:
            raise RuntimeError("No glTF/GLB importer available in this Blender installation.")
    elif ext == ".blend":
        bpy.ops.wm.open_mainfile(filepath=file_path)
    else:
        raise ValueError(f"Unsupported 3D file format: {ext}")

def export_mesh_file(bpy, file_path, target_format=None):
    """Exports selected mesh objects to destination with Roblox coordinate conventions."""
    ext = (target_format or os.path.splitext(file_path)[1][1:]).lower()
    os.makedirs(os.path.dirname(os.path.abspath(file_path)), exist_ok=True)
    
    # Ensure mesh objects are selected and active
    mesh_objs = [o for o in bpy.data.objects if o.type == 'MESH']
    if not mesh_objs:
        raise RuntimeError("No mesh objects available to export.")
    for o in bpy.data.objects:
        o.select_set(o.type == 'MESH')
    bpy.context.view_layer.objects.active = mesh_objs[0]

    if ext == "obj":
        if hasattr(bpy.ops.wm, "obj_export"):
            bpy.ops.wm.obj_export(
                filepath=file_path,
                export_selected_objects=True,
                forward_axis='NEGATIVE_Z',
                up_axis='Y',
                apply_modifiers=True,
            )
        elif hasattr(bpy.ops.export_scene, "obj"):
            bpy.ops.export_scene.obj(
                filepath=file_path,
                use_selection=True,
                axis_forward='-Z',
                axis_up='Y',
                use_mesh_modifiers=True,
                use_triangles=True,
            )
        else:
            raise RuntimeError("No OBJ exporter available in this Blender installation.")
    elif ext == "fbx":
        if hasattr(bpy.ops.export_scene, "fbx"):
            bpy.ops.export_scene.fbx(
                filepath=file_path,
                use_selection=True,
                axis_forward='-Z',
                axis_up='Y',
                apply_unit_scale=True,
                bake_space_transform=True,
                mesh_smooth_type='FACE',
            )
        else:
            raise RuntimeError("No FBX exporter available in this Blender installation.")
    elif ext in ["gltf", "glb"]:
        if hasattr(bpy.ops.export_scene, "gltf"):
            bpy.ops.export_scene.gltf(
                filepath=file_path,
                use_selection=True,
                export_format='GLB' if ext == 'glb' else 'GLTF_SEPARATE',
            )
        else:
            raise RuntimeError("No glTF/GLB exporter available in this Blender installation.")
    else:
        raise ValueError(f"Unsupported export format: {ext}")

def compute_mesh_metrics(bpy, mesh_obj):
    """Computes geometric statistics, bounding boxes, UVs, and Roblox compliance."""
    mesh = mesh_obj.data
    mesh.calc_loop_triangles()
    
    verts = [mesh_obj.matrix_world @ v.co for v in mesh.vertices]
    if verts:
        min_x = min(v.x for v in verts)
        max_x = max(v.x for v in verts)
        min_y = min(v.y for v in verts)
        max_y = max(v.y for v in verts)
        min_z = min(v.z for v in verts)
        max_z = max(v.z for v in verts)
    else:
        min_x = max_x = min_y = max_y = min_z = max_z = 0.0

    blender_bounds = {
        "min": [min_x, min_y, min_z],
        "max": [max_x, max_y, max_z],
        "dimensions": [max_x - min_x, max_y - min_y, max_z - min_z],
        "center": [(min_x + max_x) / 2.0, (min_y + max_y) / 2.0, (min_z + max_z) / 2.0],
    }

    # Blender (Z-up, +Y fwd) -> Roblox (Y-up, -Z fwd): X'=X, Y'=Z, Z'=-Y
    roblox_min = [min_x, min_z, -max_y]
    roblox_max = [max_x, max_z, -min_y]
    roblox_bounds = {
        "min": roblox_min,
        "max": roblox_max,
        "dimensions": [roblox_max[0] - roblox_min[0], roblox_max[1] - roblox_min[1], roblox_max[2] - roblox_min[2]],
        "center": [
            (roblox_min[0] + roblox_max[0]) / 2.0,
            (roblox_min[1] + roblox_max[1]) / 2.0,
            (roblox_min[2] + roblox_max[2]) / 2.0,
        ],
    }

    uv_layers = [layer.name for layer in mesh.uv_layers]
    materials_list = []
    for slot in mesh_obj.material_slots:
        mat = slot.material
        if mat:
            tex_list = []
            if mat.use_nodes and mat.node_tree:
                for node in mat.node_tree.nodes:
                    if node.type == 'TEX_IMAGE' and node.image:
                        tex_list.append({"name": node.image.name, "filepath": node.image.filepath})
            materials_list.append({"name": mat.name, "textures": tex_list})

    tri_count = len(mesh.loop_triangles)
    warnings = []
    if tri_count > 10000:
        warnings.append(f"Triangle count ({tri_count:,}) exceeds recommended budget of 10,000 for Roblox experiences.")
    if tri_count > 20000:
        warnings.append(f"Triangle count ({tri_count:,}) exceeds Roblox hard engine limit of 20,000.")
    if len(materials_list) > 1:
        warnings.append(f"Mesh has {len(materials_list)} material slots. Roblox MeshParts only render the first material slot.")
    if not uv_layers:
        warnings.append("Mesh has no UV layer. Textures and SurfaceAppearance cannot be mapped correctly.")

    return {
        "vertexCount": len(mesh.vertices),
        "edgeCount": len(mesh.edges),
        "faceCount": len(mesh.polygons),
        "triangleCount": tri_count,
        "boundingBox": {
            "blender": blender_bounds,
            "roblox": roblox_bounds,
        },
        "uvLayers": uv_layers,
        "uvLayerCount": len(uv_layers),
        "materials": materials_list,
        "materialCount": len(materials_list),
        "hasArmature": any(mod.type == 'ARMATURE' for mod in mesh_obj.modifiers),
        "hasAnimations": bool(bpy.data.actions),
        "hasVertexColors": bool(mesh.color_attributes or getattr(mesh, 'vertex_colors', None)),
        "robloxCompliance": {
            "isUnder10k": tri_count <= 10000,
            "isUnder20k": tri_count <= 20000,
            "singleMaterial": len(materials_list) <= 1,
            "hasUV": len(uv_layers) >= 1,
            "warnings": warnings,
        },
    }

# ==========================================
# Task Handlers
# ==========================================

def handle_inspect_mesh(bpy, params, allowed_roots):
    file_path = validate_sandboxed_path(params["filePath"], allowed_roots)
    if not os.path.exists(file_path):
        raise FileNotFoundError(f"File not found: {file_path}")

    clear_blender_scene(bpy)
    import_mesh_file(bpy, file_path)

    mesh_objs = [o for o in bpy.data.objects if o.type == 'MESH']
    if not mesh_objs:
        raise RuntimeError("No mesh objects found in imported file.")

    primary_mesh = mesh_objs[0]
    metrics = compute_mesh_metrics(bpy, primary_mesh)
    file_size = os.path.getsize(file_path)

    return {
        "filePath": file_path,
        "format": os.path.splitext(file_path)[1].lstrip(".").lower(),
        "fileSizeBytes": file_size,
        "objectCount": len(bpy.data.objects),
        "meshCount": len(mesh_objs),
        **metrics,
        "success": True,
    }

def handle_convert_format(bpy, params, allowed_roots):
    input_path = validate_sandboxed_path(params["inputPath"], allowed_roots)
    output_path = validate_sandboxed_path(params["outputPath"], allowed_roots)
    target_format = params.get("targetFormat", "fbx").lower()
    pivot_mode = params.get("normalizePivot", "bottom_center")
    apply_transforms = params.get("applyTransforms", True)

    if not os.path.exists(input_path):
        raise FileNotFoundError(f"Input file not found: {input_path}")

    clear_blender_scene(bpy)
    import_mesh_file(bpy, input_path)

    mesh_objs = [o for o in bpy.data.objects if o.type == 'MESH']
    if not mesh_objs:
        raise RuntimeError("No mesh objects found in file.")

    primary_mesh = mesh_objs[0]
    bpy.context.view_layer.objects.active = primary_mesh
    primary_mesh.select_set(True)

    if apply_transforms:
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

    if pivot_mode == "bottom_center":
        verts = [primary_mesh.matrix_world @ v.co for v in primary_mesh.data.vertices]
        if verts:
            min_z = min(v.z for v in verts)
            center_x = (min(v.x for v in verts) + max(v.x for v in verts)) / 2.0
            center_y = (min(v.y for v in verts) + max(v.y for v in verts)) / 2.0
            for v in primary_mesh.data.vertices:
                v.co.x -= center_x
                v.co.y -= center_y
                v.co.z -= min_z
            primary_mesh.data.update()
    elif pivot_mode == "geometric_center":
        verts = [primary_mesh.matrix_world @ v.co for v in primary_mesh.data.vertices]
        if verts:
            center_x = (min(v.x for v in verts) + max(v.x for v in verts)) / 2.0
            center_y = (min(v.y for v in verts) + max(v.y for v in verts)) / 2.0
            center_z = (min(v.z for v in verts) + max(v.z for v in verts)) / 2.0
            for v in primary_mesh.data.vertices:
                v.co.x -= center_x
                v.co.y -= center_y
                v.co.z -= center_z
            primary_mesh.data.update()

    export_mesh_file(bpy, output_path, target_format)
    file_size = os.path.getsize(output_path) if os.path.exists(output_path) else 0

    return {
        "outputPath": output_path,
        "format": target_format,
        "fileSizeBytes": file_size,
        "appliedTransforms": {
            "scaleMultiplier": params.get("scaleMultiplier", 1.0),
            "pivotMode": pivot_mode,
            "appliedTransforms": apply_transforms,
            "robloxAxes": {"axis_forward": "-Z", "axis_up": "Y"},
        },
        "success": True,
    }

def handle_decimate_mesh(bpy, params, allowed_roots):
    input_path = validate_sandboxed_path(params["inputPath"], allowed_roots)
    output_path = validate_sandboxed_path(params["outputPath"], allowed_roots)
    target_triangles = params.get("targetTriangles")
    ratio = params.get("ratio")
    preserve_uvs = params.get("preserveUVs", True)
    preserve_normals = params.get("preserveNormals", True)

    if not os.path.exists(input_path):
        raise FileNotFoundError(f"Input file not found: {input_path}")

    clear_blender_scene(bpy)
    import_mesh_file(bpy, input_path)

    mesh_objs = [o for o in bpy.data.objects if o.type == 'MESH']
    if not mesh_objs:
        raise RuntimeError("No mesh objects found to decimate.")

    obj = mesh_objs[0]
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)

    mesh = obj.data
    mesh.calc_loop_triangles()
    orig_tris = len(mesh.loop_triangles)
    orig_verts = len(mesh.vertices)

    if ratio is None and target_triangles:
        ratio = min(1.0, max(0.01, float(target_triangles) / float(max(1, orig_tris))))
    elif ratio is None:
        ratio = 0.5
    else:
        ratio = max(0.01, min(1.0, float(ratio)))

    mod = obj.modifiers.new(name="RobloxDecimate", type='DECIMATE')
    mod.decimate_type = 'COLLAPSE'
    mod.ratio = ratio
    mod.use_collapse_triangulate = True
    if preserve_uvs:
        mod.use_symmetry = False

    bpy.ops.object.modifier_apply(modifier=mod.name)

    if preserve_normals:
        if hasattr(mesh, 'use_auto_smooth'):
            mesh.use_auto_smooth = True
            mesh.auto_smooth_angle = math.radians(60)
        else:
            bpy.ops.object.shade_smooth()

    mesh.calc_loop_triangles()
    dec_tris = len(mesh.loop_triangles)
    dec_verts = len(mesh.vertices)

    export_mesh_file(bpy, output_path)

    return {
        "outputPath": output_path,
        "originalTriangles": orig_tris,
        "originalVertices": orig_verts,
        "decimatedTriangles": dec_tris,
        "decimatedVertices": dec_verts,
        "reductionRatio": round(float(dec_tris) / float(max(1, orig_tris)), 4),
        "targetMet": (target_triangles is None) or (dec_tris <= int(target_triangles)),
        "success": True,
    }

def handle_generate_collision(bpy, bmesh, params, allowed_roots):
    input_path = validate_sandboxed_path(params["inputPath"], allowed_roots)
    output_path = validate_sandboxed_path(params["outputPath"], allowed_roots)
    col_type = params.get("collisionType", "convex_hull").lower()
    max_tris = params.get("maxCollisionTriangles", 64)

    if not os.path.exists(input_path):
        raise FileNotFoundError(f"Input file not found: {input_path}")

    clear_blender_scene(bpy)
    import_mesh_file(bpy, input_path)

    mesh_objs = [o for o in bpy.data.objects if o.type == 'MESH']
    if not mesh_objs:
        raise RuntimeError("No mesh objects found to generate collision.")

    obj = mesh_objs[0]
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)

    if col_type == "convex_hull":
        bm = bmesh.new()
        bm.from_mesh(obj.data)
        bmesh.ops.convex_hull(bm, input=bm.verts)
        bm.to_mesh(obj.data)
        bm.free()
        recommended = "Hull"
    elif col_type == "bounding_box":
        verts = [obj.matrix_world @ v.co for v in obj.data.vertices]
        min_x = min(v.x for v in verts)
        max_x = max(v.x for v in verts)
        min_y = min(v.y for v in verts)
        max_y = max(v.y for v in verts)
        min_z = min(v.z for v in verts)
        max_z = max(v.z for v in verts)
        
        bm = bmesh.new()
        bmesh.ops.create_cube(
            bm,
            size=1.0,
            matrix=mathutils.Matrix.Translation(((min_x + max_x)/2, (min_y + max_y)/2, (min_z + max_z)/2)) @
                   mathutils.Matrix.Diagonal(((max_x - min_x), (max_y - min_y), (max_z - min_z), 1.0))
        )
        bm.to_mesh(obj.data)
        bm.free()
        recommended = "Box"
    else:  # simplified_mesh
        mod = obj.modifiers.new(name="CollisionDecimate", type='DECIMATE')
        mod.decimate_type = 'COLLAPSE'
        mesh = obj.data
        mesh.calc_loop_triangles()
        current_tris = len(mesh.loop_triangles)
        mod.ratio = max(0.01, min(1.0, float(max_tris) / float(max(1, current_tris))))
        mod.use_collapse_triangulate = True
        bpy.ops.object.modifier_apply(modifier=mod.name)
        recommended = "PreciseConvexDecomposition"

    obj.data.calc_loop_triangles()
    tri_count = len(obj.data.loop_triangles)
    vert_count = len(obj.data.vertices)

    export_mesh_file(bpy, output_path, target_format="obj")

    return {
        "outputPath": output_path,
        "collisionType": col_type,
        "triangleCount": tri_count,
        "vertexCount": vert_count,
        "recommendedRobloxFidelity": recommended,
        "success": True,
    }

def handle_export_roblox_package(bpy, bmesh, params, allowed_roots):
    input_mesh = validate_sandboxed_path(params["inputMeshPath"], allowed_roots)
    output_dir = validate_sandboxed_path(params["outputDir"], allowed_roots)
    asset_name = params.get("assetName", "RobloxAsset")
    mesh_format = params.get("format", "fbx").lower()
    decimate_budget = params.get("decimateToBudget")
    gen_collision = params.get("generateCollision", True)
    col_type = params.get("collisionType", "convex_hull")
    tex_dir = params.get("textureDirectory")
    attribution = params.get("attribution", {})

    os.makedirs(output_dir, exist_ok=True)

    # 1. Process & Export Visual Mesh
    clear_blender_scene(bpy)
    import_mesh_file(bpy, input_mesh)
    mesh_objs = [o for o in bpy.data.objects if o.type == 'MESH']
    if not mesh_objs:
        raise RuntimeError("No mesh objects found.")
    obj = mesh_objs[0]

    if decimate_budget:
        obj.data.calc_loop_triangles()
        cur_tris = len(obj.data.loop_triangles)
        if cur_tris > decimate_budget:
            mod = obj.modifiers.new(name="BudgetDecimate", type='DECIMATE')
            mod.decimate_type = 'COLLAPSE'
            mod.ratio = max(0.01, min(1.0, float(decimate_budget) / float(cur_tris)))
            mod.use_collapse_triangulate = True
            bpy.ops.object.modifier_apply(modifier=mod.name)

    visual_filename = f"{asset_name}.{mesh_format}"
    visual_path = os.path.join(output_dir, visual_filename)
    export_mesh_file(bpy, visual_path, target_format=mesh_format)
    metrics = compute_mesh_metrics(bpy, obj)

    # 2. Collision Mesh
    collision_filename = None
    if gen_collision:
        collision_filename = f"{asset_name}_collision.obj"
        col_path = os.path.join(output_dir, collision_filename)
        bm = bmesh.new()
        bm.from_mesh(obj.data)
        bmesh.ops.convex_hull(bm, input=bm.verts)
        bm.to_mesh(obj.data)
        bm.free()
        export_mesh_file(bpy, col_path, target_format="obj")

    # 3. PBR Textures
    exported_textures = {}
    tex_dest_dir = os.path.join(output_dir, "textures")
    if tex_dir and os.path.exists(tex_dir):
        os.makedirs(tex_dest_dir, exist_ok=True)
        for fname in os.listdir(tex_dir):
            low = fname.lower()
            src_f = os.path.join(tex_dir, fname)
            if not os.path.isfile(src_f):
                continue
            dst_f = os.path.join(tex_dest_dir, fname)
            rel_tex = f"textures/{fname}"
            if any(k in low for k in ["color", "albedo", "diffuse", "basecolor"]):
                shutil.copy2(src_f, dst_f)
                exported_textures["colorMap"] = rel_tex
            elif any(k in low for k in ["normal", "norm", "nrm"]):
                shutil.copy2(src_f, dst_f)
                exported_textures["normalMap"] = rel_tex
            elif any(k in low for k in ["roughness", "rough", "rgh"]):
                shutil.copy2(src_f, dst_f)
                exported_textures["roughnessMap"] = rel_tex
            elif any(k in low for k in ["metalness", "metallic", "metal"]):
                shutil.copy2(src_f, dst_f)
                exported_textures["metalnessMap"] = rel_tex

    # 4. Generate Manifest JSON
    manifest = {
        "schemaVersion": "1.0",
        "name": asset_name,
        "generator": "open-ro v0.1.0 (Blender 3D Asset Pipeline)",
        "createdAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "files": {
            "visualMesh": visual_filename,
            "collisionMesh": collision_filename,
            "textures": exported_textures,
        },
        "geometry": {
            "triangleCount": metrics["triangleCount"],
            "vertexCount": metrics["vertexCount"],
            "boundingBox": {
                "dimensions": metrics["boundingBox"]["roblox"]["dimensions"],
                "unit": "studs",
            },
        },
        "robloxSettings": {
            "recommendedScale": 1.0,
            "collisionFidelity": "Hull" if gen_collision else "Default",
            "useSurfaceAppearance": len(exported_textures) > 0,
        },
        "attribution": {
            "author": attribution.get("author", "Open-Ro Creator"),
            "license": attribution.get("license", "CC-BY 4.0"),
            "sourceUrl": attribution.get("sourceUrl", ""),
            "notes": attribution.get("notes", "Optimized for Roblox"),
        },
    }

    manifest_path = os.path.join(output_dir, "roblox-asset.json")
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)

    all_exported = [visual_path]
    if collision_filename:
        all_exported.append(os.path.join(output_dir, collision_filename))
    for tpath in exported_textures.values():
        all_exported.append(os.path.join(output_dir, tpath))
    all_exported.append(manifest_path)

    return {
        "manifestPath": manifest_path,
        "packageDir": output_dir,
        "assetName": asset_name,
        "manifest": manifest,
        "exportedFiles": all_exported,
        "success": True,
    }

# ==========================================
# Main CLI Dispatcher
# ==========================================

def main():
    start_time = time.time()
    
    # Parse CLI flags after '--'
    cli_args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    
    task_name = None
    params = {}
    params_file = None
    output_json = None
    allowed_roots = []

    i = 0
    while i < len(cli_args):
        arg = cli_args[i]
        if arg == "--task" and i + 1 < len(cli_args):
            task_name = cli_args[i + 1]
            i += 2
        elif arg == "--params" and i + 1 < len(cli_args):
            params = json.loads(cli_args[i + 1])
            i += 2
        elif arg == "--params-file" and i + 1 < len(cli_args):
            params_file = cli_args[i + 1]
            i += 2
        elif arg == "--output-json" and i + 1 < len(cli_args):
            output_json = cli_args[i + 1]
            i += 2
        elif arg == "--allowed-roots" and i + 1 < len(cli_args):
            try:
                allowed_roots = json.loads(cli_args[i + 1])
            except Exception:
                allowed_roots = [cli_args[i + 1]]
            i += 2
        else:
            i += 1

    if params_file and os.path.exists(params_file):
        with open(params_file, "r", encoding="utf-8") as pf:
            params = json.load(pf)

    # Lazy-load bpy inside Blender runtime
    try:
        import bpy
        import bmesh
        import mathutils
    except ImportError as ie:
        err_msg = f"Blender bpy/bmesh module unavailable: {ie}"
        print(f"[Open-Ro Error] {err_msg}", file=sys.stderr)
        res = {"success": False, "error": err_msg, "durationMs": int((time.time() - start_time) * 1000)}
        emit_result(res, output_json)
        sys.exit(1)

    result = {"success": False}
    try:
        if task_name == "inspect_mesh":
            result = handle_inspect_mesh(bpy, params, allowed_roots)
        elif task_name == "convert_format":
            result = handle_convert_format(bpy, params, allowed_roots)
        elif task_name == "decimate_mesh":
            result = handle_decimate_mesh(bpy, params, allowed_roots)
        elif task_name == "generate_collision":
            result = handle_generate_collision(bpy, bmesh, params, allowed_roots)
        elif task_name == "export_roblox_package":
            result = handle_export_roblox_package(bpy, bmesh, params, allowed_roots)
        else:
            raise ValueError(f"Unknown or unspecified 3D task: {task_name}")
        result["durationMs"] = int((time.time() - start_time) * 1000)
    except Exception as exc:
        trace = traceback.format_exc()
        print(f"[Open-Ro 3D Pipeline Error] {trace}", file=sys.stderr)
        result = {
            "success": False,
            "error": str(exc),
            "traceback": trace,
            "durationMs": int((time.time() - start_time) * 1000),
        }

    emit_result(result, output_json)
    sys.exit(0 if result.get("success") else 1)

def emit_result(result_dict, output_json_path):
    serialized = json.dumps(result_dict, indent=2)
    if output_json_path:
        try:
            with open(output_json_path, "w", encoding="utf-8") as f:
                f.write(serialized)
        except Exception as we:
            print(f"[Open-Ro Warning] Failed writing output-json: {we}", file=sys.stderr)
    print(f"\\n{SENTINEL_START}")
    print(serialized)
    print(f"{SENTINEL_END}\\n")

if __name__ == "__main__":
    main()
`;

export function getBlenderPipelineScript(): string {
  return OPENRO_3D_PIPELINE_SCRIPT;
}
