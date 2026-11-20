using System;
using System.Runtime.InteropServices;
using UnityEngine;

namespace Glim
{
    public static class UVPacking
    {
        const string DLL = "glim";

        [DllImport(DLL)]
        public static extern IntPtr uvpacker_create(uint width, uint height, uint iterations, [MarshalAs(UnmanagedType.I1)] bool bruteForce, [MarshalAs(UnmanagedType.I1)] bool holeFilling, [MarshalAs(UnmanagedType.I1)] bool splitCharts, float world_scale_exponent);

        [DllImport(DLL)]
        public static extern void uvpacker_destroy(IntPtr handle);

        [DllImport(DLL)]
        public static extern unsafe void uvpacker_add_mesh(IntPtr handle, Vector3* positions, uint positionCount, Vector2* uvs, uint uvCount, int* indices, uint indexCount, float scaleMultiplier, uint meshId);

        public enum SplitMode : uint
        {
            Default = 0,     // use splitCharts from uvpacker_create
            ScaleOffset = 1, // whole mesh is one chart (e.g. GPU instanced materials)
            Split = 2,       // split into UV islands
        }

        // Same as uvpacker_add_mesh, with a per-mesh override of splitCharts.
        [DllImport(DLL)]
        public static extern unsafe void uvpacker_add_mesh_ex(IntPtr handle, Vector3* positions, uint positionCount, Vector2* uvs, uint uvCount, int* indices, uint indexCount, float scaleMultiplier, uint meshId, SplitMode splitMode);

        [DllImport(DLL)]
        [return: MarshalAs(UnmanagedType.I1)]
        public static extern bool uvpacker_pack(IntPtr handle);

        // Called on the thread that called uvpacker_pack_progress at the start of every iteration. Return true to cancel.
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        public delegate bool ProgressCallback(uint iteration, uint iterationCount);

        // Same as uvpacker_pack with progress reporting. Returns false if packing failed or was cancelled.
        [DllImport(DLL)]
        [return: MarshalAs(UnmanagedType.I1)]
        public static extern bool uvpacker_pack_progress(IntPtr handle, ProgressCallback callback);

        // Only valid for meshes packed as a single chart (added without splitting).
        // x,y = scale, z,w = offset
        [DllImport(DLL)]
        public static extern Vector4 uvpacker_get_scale_offset(IntPtr handle, uint chart);

        [DllImport(DLL)]
        public static extern uint uvpacker_get_mesh_vertex_count(IntPtr handle, uint meshId);

        [DllImport(DLL)]
        [return: MarshalAs(UnmanagedType.I1)]
        public static extern unsafe bool uvpacker_get_mesh_uvs(IntPtr handle, uint meshId, Vector2* outUvs, uint outCount);

        [DllImport(DLL)]
        public static extern float uvpacker_get_coverage(IntPtr handle);

        /// <summary>
        /// Returns the packed UVs for a mesh, index-compatible with the mesh that was added
        /// (assign directly to mesh.uv). Works with and without chart splitting.
        /// Returns null if pack hasn't succeeded or the mesh id is unknown.
        /// </summary>
        public static unsafe Vector2[] GetMeshUvs(IntPtr handle, uint meshId)
        {
            uint count = uvpacker_get_mesh_vertex_count(handle, meshId);
            if (count == 0)
                return null;

            var result = new Vector2[count];
            fixed (Vector2* p = result)
            {
                if (!uvpacker_get_mesh_uvs(handle, meshId, p, count))
                    return null;
            }

            return result;
        }
    }
}
