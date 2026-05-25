using System;
using System.Runtime.InteropServices;
using Unity.Collections.LowLevel.Unsafe;
using UnityEngine;
using UnityEngine.Rendering;

namespace stilb
{
    public static class Bindings
    {
        public enum CoordinateSystem : uint
        {
            Default = 0,
            Unity = 1,
        }

        public enum TextureSamplerFilter : uint
        {
            Nearest = 0,
            Linear = 1,
        }


        [StructLayout(LayoutKind.Sequential)]
        public struct StilbConfig
        {
            public readonly CoordinateSystem coordinate_system;

            [MarshalAs(UnmanagedType.I1)] public readonly bool is_preview;
            [MarshalAs(UnmanagedType.I1)] public readonly bool vulkan_validation_layers;
            [MarshalAs(UnmanagedType.I1)] public readonly bool seams_debug;
            public readonly uint throttle_preview_ms;
            public readonly LightmapSettings preview_settings;

            public readonly Vector3 camera_position;
            public readonly Vector3 camera_forward;

            public readonly ReadbackCallback callback;
            public readonly ReadbackProbesCallback probes_callback;

            public readonly TextureSamplerFilter texture_filter;
            public readonly uint probe_samples;
            public readonly uint probe_bounces;
            public readonly uint light_falloff;

            public StilbConfig(CoordinateSystem coordinate_system,
                               bool is_preview,
                               uint throttle_preview_ms,
                               LightmapSettings preview_settings,
                               Vector3 camera_position,
                               Vector3 camera_forward,
                               TextureSamplerFilter texture_filter,
                               uint probe_samples,
                               uint probe_bounces,
                               LightFalloffType falloff)
            {
                this.coordinate_system = coordinate_system;
                this.is_preview = is_preview;
                this.throttle_preview_ms = throttle_preview_ms;
                this.preview_settings = preview_settings;
                this.camera_position = camera_position;
                this.camera_forward = camera_forward;
                this.callback = Bake.OnReadback;
                this.probes_callback = Bake.OnReadbackProbes;
                this.texture_filter = texture_filter;
                this.probe_samples = probe_samples;
                this.probe_bounces = probe_bounces;
                this.vulkan_validation_layers = false;
                this.seams_debug = false;

                var currentPipeline = GraphicsSettings.currentRenderPipeline;
                uint autoFalloff = 0;
                if (currentPipeline == null) // BuiltIn
                {
                    autoFalloff = 1;
                }

                this.light_falloff = falloff switch
                {
                    LightFalloffType.Auto => autoFalloff,
                    LightFalloffType.InverseSquare => 0,
                    LightFalloffType.UnityBuiltIn => 1,
                    _ => 0,
                };
            }
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct LightmapSettings
        {
            public readonly uint width;
            public readonly uint height;

            public readonly uint max_samples;
            public readonly uint bounce_count;

            [MarshalAs(UnmanagedType.I1)] public readonly bool dilate;
            [MarshalAs(UnmanagedType.I1)] public readonly bool denoise;
            [MarshalAs(UnmanagedType.I1)] public readonly bool fix_seams;

            public LightmapSettings(uint width, uint height, uint max_samples, uint bounce_count, bool dilate, bool denoise, bool fix_seams)
            {
                this.width = width;
                this.height = height;
                this.max_samples = max_samples;
                this.bounce_count = bounce_count;
                this.dilate = dilate;
                this.denoise = denoise;
                this.fix_seams = fix_seams;
            }

            public LightmapSettings(LightmapGroup group) :
                this(group.resolution, group.resolution, group.maxSamples, group.bounceCount, group.dilate, group.denoise, group.fixSeams)
            {
                return;
            }

        }

        const string DLL_NAME = "stilb";

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr app_new(StilbConfig config);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern int app_run(IntPtr app);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern int app_destroy(IntPtr app);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern int app_add_mesh(IntPtr app, Mesh mesh);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern int app_add_probe(IntPtr app, Vector3 position);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static extern int app_add_light(IntPtr app, Light light);

        [DllImport(DLL_NAME, CallingConvention = CallingConvention.Cdecl)]
        public static unsafe extern int app_add_lightmap_group(IntPtr app, LightmapSettings settings, byte* albedoPixels, uint albedoPixelsLength, float* emissionPixels, uint emissionPixelsLength);

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

            [MarshalAs(UnmanagedType.I1)] public bool backface_gi;
            [MarshalAs(UnmanagedType.I1)] public bool transparent;
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
            public Vector3 position;
            public LightType ty;

            public Vector3 direction;
            public float range;

            public Vector3 color;
            public float shadow_radius_or_angle;
        }

        [StructLayout(LayoutKind.Sequential, Pack = 16)]
        public struct SHProbe
        {
            public Vector3 Position;
            public uint Pad0;

            // L0
            public Vector3 L0;
            public uint Pad1;

            // L1
            public Vector3 L1_1;
            public uint Pad2;
            public Vector3 L10;
            public uint Pad3;
            public Vector3 L11;
            public uint Pad4;

            // L2
            public Vector3 L2_2;
            public uint Pad5;
            public Vector3 L2_1;
            public uint Pad6;
            public Vector3 L20;
            public uint Pad7;
            public Vector3 L21;
            public uint Pad8;
            public Vector3 L22;
            public uint Pad9;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct ReadbackProbeData
        {
            public IntPtr probes;
            public uint probes_count;

            public unsafe SHProbe[] GetProbes()
            {
                int count = (int)probes_count;
                if (count == 0 || probes == IntPtr.Zero)
                {
                    return Array.Empty<SHProbe>();
                }

                SHProbe[] managedArray = new SHProbe[count];

                int structSize = UnsafeUtility.SizeOf<SHProbe>();
                long byteCount = count * structSize;

                fixed (SHProbe* destPtr = managedArray)
                {
                    Buffer.MemoryCopy((void*)probes, destPtr, byteCount, byteCount);
                }

                return managedArray;
            }
        }

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        public delegate void ReadbackProbesCallback(ReadbackProbeData data);

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        public delegate void ReadbackCallback(ReadbackData data);

        [StructLayout(LayoutKind.Sequential)]
        public struct ReadbackData
        {
            public uint group_index;
            public uint ty;
            public uint width;
            public uint height;
            public IntPtr pixels;
            public uint pixels_count;

            public unsafe Color[] GetPixels()
            {
                if (pixels == IntPtr.Zero || pixels_count == 0)
                    return Array.Empty<Color>();

                int colorCount = (int)pixels_count / 4;
                Color[] managedArray = new Color[colorCount];

                fixed (Color* destPtr = managedArray)
                {
                    long byteCount = pixels_count * sizeof(float);
                    Buffer.MemoryCopy((void*)pixels, destPtr, byteCount, byteCount);
                }

                return managedArray;
            }
        }
    }
}
