"""Props built from scratch in Blender (imported by `blender_prop.py --build NAME`): simple shapes
dressed with the game's own CC0 surface textures, for the things no CC0 pack had. Each builder
takes the folder of surface texture sets (crates/frontline/assets/textures) and returns one mesh
object in metres, z up; `blender_prop.py` grounds, decimates and exports it.
"""
import math
import os
import random

import bmesh
import bpy


def surface_material(name: str, textures: str, surface: str, tint=(1.0, 1.0, 1.0)):
    """A Principled material from a surface's colour, normal and roughness maps, tinted."""
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    nodes, links = material.node_tree.nodes, material.node_tree.links
    shader = nodes["Principled BSDF"]

    def image(file: str, colour: bool):
        node = nodes.new("ShaderNodeTexImage")
        node.image = bpy.data.images.load(os.path.join(textures, surface, file))
        if not colour:
            node.image.colorspace_settings.name = "Non-Color"
        return node

    mix = nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    links.new(image("color.jpg", True).outputs["Color"], mix.inputs["A"])
    mix.inputs["B"].default_value = (*tint, 1.0)
    links.new(mix.outputs["Result"], shader.inputs["Base Color"])
    normal = nodes.new("ShaderNodeNormalMap")
    links.new(image("normal.jpg", False).outputs["Color"], normal.inputs["Color"])
    links.new(normal.outputs["Normal"], shader.inputs["Normal"])
    links.new(image("roughness.jpg", False).outputs["Color"], shader.inputs["Roughness"])
    shader.inputs["Metallic"].default_value = 0.0
    return material


def flat_material(name: str, colour, roughness: float = 0.8):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    shader = material.node_tree.nodes["Principled BSDF"]
    shader.inputs["Base Color"].default_value = (*colour, 1.0)
    shader.inputs["Roughness"].default_value = roughness
    return material


def box(size, location, material, bevel: float = 0.0, taper: float = 1.0):
    """A box of `size` (x, y, z) centred on `location`; `taper` scales its bottom face in x and
    y (a skip narrows towards the ground)."""
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=location)
    obj = bpy.context.view_layer.objects.active
    obj.scale = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if taper != 1.0:
        for vertex in obj.data.vertices:
            if vertex.co.z < 0.0:
                vertex.co.x *= taper
                vertex.co.y *= taper
    if bevel > 0.0:
        modifier = obj.modifiers.new("bevel", "BEVEL")
        modifier.width = bevel
        modifier.segments = 1
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    obj.data.materials.append(material)
    return obj


def cylinder(radius: float, depth: float, location, rotation, material, vertices: int = 12):
    bpy.ops.mesh.primitive_cylinder_add(radius=radius, depth=depth, vertices=vertices, location=location, rotation=rotation)
    obj = bpy.context.view_layer.objects.active
    obj.data.materials.append(material)
    return obj


def join(objects, tile: float = 1.0):
    """Joins the objects into one, box-projects its UVs at `tile` metres per repeat and shades it
    smooth where faces meet at a shallow angle."""
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.cube_project(cube_size=tile, scale_to_bounds=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    bpy.ops.object.shade_auto_smooth(angle=math.radians(40))
    return obj


def dumpster(textures: str):
    """A steel skip with two lids, 2.0 x 1.2 m and 1.3 m tall."""
    green = surface_material("dumpster_body", textures, "metal_green")
    dark = flat_material("dumpster_lid", (0.05, 0.055, 0.06), 0.6)
    steel = surface_material("dumpster_steel", textures, "metal_rust")
    parts = [
        box((2.0, 1.2, 1.0), (0.0, 0.0, 0.62), green, bevel=0.015, taper=0.9),
        box((2.06, 1.26, 0.06), (0.0, 0.0, 1.13), green, bevel=0.01),  # the rim
        box((0.12, 0.5, 0.16), (-1.03, 0.0, 0.8), steel, bevel=0.01),  # the lifting sleeves
        box((0.12, 0.5, 0.16), (1.03, 0.0, 0.8), steel, bevel=0.01),
    ]
    for x in (-0.5, 0.5):
        lid = box((0.98, 1.24, 0.04), (x, 0.0, 1.21), dark, bevel=0.008)
        lid.rotation_euler = (math.radians(7.0), 0.0, 0.0)
        parts.append(lid)
    for x in (-0.8, 0.8):
        for y in (-0.45, 0.45):
            parts.append(cylinder(0.09, 0.06, (x, y, 0.09), (0.0, math.radians(90.0), 0.0), dark))
    return join(parts, tile=2.0)


def pallet(textures: str):
    """A wooden pallet, 1.2 x 0.8 m and 0.144 m tall."""
    wood = surface_material("pallet_wood", textures, "wood", tint=(0.8, 0.74, 0.66))
    parts = []
    for i in range(5):  # top boards
        parts.append(box((1.2, 0.1, 0.022), (0.0, -0.35 + i * 0.175, 0.133), wood))
    for x in (-0.55, 0.0, 0.55):  # blocks under three bearers
        parts.append(box((0.1, 0.8, 0.022), (x, 0.0, 0.111), wood))
        for y in (-0.35, 0.0, 0.35):
            parts.append(box((0.1, 0.1, 0.078), (x, y, 0.061), wood))
    for y in (-0.35, 0.0, 0.35):  # bottom boards
        parts.append(box((1.2, 0.1, 0.022), (0.0, y, 0.011), wood))
    return join(parts, tile=1.0)


def sandbags(textures: str):
    """A sandbag wall, about 2.0 m long, 0.5 m deep and 0.9 m tall: chest-high cover."""
    sack = surface_material("sandbag", textures, "sand", tint=(0.62, 0.58, 0.46))
    rng = random.Random(7)
    parts = []
    length, depth, height = 0.66, 0.36, 0.2
    for row in range(5):
        count = 3 if row % 2 == 0 else 2
        start = -(count - 1) * length / 2
        for i in range(count):
            bpy.ops.mesh.primitive_uv_sphere_add(segments=12, ring_count=6, radius=0.5)
            bag = bpy.context.view_layer.objects.active
            bag.scale = (length * 1.08, depth * (1.0 + rng.uniform(-0.06, 0.08)), height * 1.25)
            bag.location = (start + i * length + rng.uniform(-0.02, 0.02), rng.uniform(-0.03, 0.03), height * (row + 0.5))
            bag.rotation_euler = (0.0, 0.0, rng.uniform(-0.06, 0.06))
            bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
            # Flatten top and bottom so the bags sit on each other like filled sacks.
            mesh = bmesh.new()
            mesh.from_mesh(bag.data)
            low, high = height * row + 0.01, height * (row + 1) + 0.02
            for vertex in mesh.verts:
                vertex.co.z = min(max(vertex.co.z, low), high)
            mesh.to_mesh(bag.data)
            mesh.free()
            bag.data.materials.append(sack)
            parts.append(bag)
    return join(parts, tile=0.8)


BUILDS = {"dumpster": dumpster, "pallet": pallet, "sandbags": sandbags}
