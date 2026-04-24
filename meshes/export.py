import os
import struct

import bpy

# Use the active object
obj = bpy.context.active_object
mesh = obj.to_mesh()
mesh.calc_loop_triangles()

folder = bpy.path.abspath("//")
filepath = os.path.join(folder, "export.bin")

with open(filepath, "wb") as f:
    for tri in mesh.loop_triangles:
        for loop_idx in tri.loops:
            # 1. Get the loop and the vertex it points to
            loop = mesh.loops[loop_idx]
            vert = mesh.vertices[loop.vertex_index]

            # 2. Extract data (Position, Normal, UV)
            v = vert.co
            n = loop.normal  # Use loop normal for proper sharp edges

            uv_data = [0.0, 0.0]
            if mesh.uv_layers.active:
                uv_data = mesh.uv_layers.active.data[loop_idx].uv

            # 3. MATCH YOUR STRUCT:
            # pos(3f), uv_x(1f), norm(3f), uv_y(1f)
            # Total 8 floats = 32 bytes
            data = struct.pack(
                "8f",
                v.x,
                v.y,
                v.z,  # position
                uv_data[0],  # uv_x
                n.x,
                n.y,
                n.z,  # normal
                uv_data[1],  # uv_y
            )
            f.write(data)

print(f"Successfully exported {len(mesh.loop_triangles) * 3} vertices to {filepath}")
