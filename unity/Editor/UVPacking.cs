using System;
using System.Runtime.InteropServices;
using UnityEngine;

namespace stilb
{
    public static class UVPacking
    {
        const string DLL = "stilb";

        [DllImport(DLL)]
        public static extern IntPtr uvpacker_create(uint width, uint height, uint iterations, [MarshalAs(UnmanagedType.I1)] bool bruteForce);

        [DllImport(DLL)]
        public static extern void uvpacker_destroy(IntPtr handle);

        [DllImport(DLL)]
        public static extern unsafe void uvpacker_add_mesh(IntPtr handle, Vector3* positions, uint positionCount, Vector2* uvs, uint uvCount, int* indices, uint indexCount, float scaleMultiplier, uint meshId);

        [DllImport(DLL)]
        [return: MarshalAs(UnmanagedType.I1)]
        public static extern bool uvpacker_pack(IntPtr handle);

        [DllImport(DLL)]
        public static extern Vector4 uvpacker_get_scale_offset(IntPtr handle, uint chart);

    }
}
