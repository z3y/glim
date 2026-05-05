using System;
using System.Runtime.InteropServices;
using UnityEngine;

namespace stilb
{
    public static class Bindings
    {

        [StructLayout(LayoutKind.Sequential)]
        public struct StilbConfig
        {
            [MarshalAs(UnmanagedType.I1)]
            public bool is_preview;

            public uint preview_width;
            public uint preview_height;

            public Vector3 camera_position;
            public Vector3 camera_forward;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct LightmapSettings
        {
            public uint width;
            public uint height;

            public uint max_samples;
            public uint bounce_count;

            [MarshalAs(UnmanagedType.I1)]
            public bool denoise;
        }

        const string DLL_NAME = "stilb";

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr app_new(StilbConfig config);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern void app_run(IntPtr app);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern void app_destroy(IntPtr app);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern void app_add_mesh(IntPtr app, Mesh mesh);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern void app_add_light(IntPtr app, Light light);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static unsafe extern void app_add_lightmap_group(IntPtr app, LightmapSettings settings, byte* albedoPixels, uint albedoPixelsLength, float* emissionPixels, uint emissionPixelsLength);

        [StructLayout(LayoutKind.Sequential)]
        public unsafe struct Mesh
        {
            public Vector3* vertices;
            public Vector3* normals;
            public Vector2* uvs;
            public uint* indices;

            public uint vertices_length;
            public uint indices_length;
            public uint lightmap_group;
        }

        public enum LightType : uint
        {
            Directional = 0,
            Point = 1,
            Spot = 2,
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct Light
        {
            public LightType ty;
            public Vector3 position;

            public Vector3 direction;
            public float range;

            public Vector3 color;
            public float shadow_radius_or_angle;
        }
    }
}