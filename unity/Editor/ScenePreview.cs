using System;
using System.Collections.Generic;
using Unity.Collections.LowLevel.Unsafe;
using UnityEditor;
using UnityEditor.Overlays;
using UnityEngine;

namespace Glim
{
    /// <summary>
    /// Shows the path traced preview as a full screen quad in the scene view, rendered from the scene view camera.
    /// Frames are rendered by glim on its own Vulkan device and read back into CPU memory as linear RGBA16F,
    /// then uploaded into a regular Texture2D, so it works with any editor graphics API.
    /// The quad is drawn in the overlay queue, before post processing, so the scene view post processing applies to it.
    /// </summary>
    public static class ScenePreview
    {
        const double ResizeDelay = 0.15;
        const string ScalePrefKey = "Glim.PreviewWindow.Scale";
        const string ShaderPath = "Packages/io.github.z3y.glim/Editor/ScenePreview.shader";

        internal static readonly float[] Scales = { 1f, 0.75f, 0.5f, 0.25f };
        internal static readonly string[] ScaleNames = { "100%", "75%", "50%", "25%" };

        // only the first frame, the scene view size takes over right after
        public const uint InitialWidth = 512;
        public const uint InitialHeight = 512;

        // not remembered, reset with the domain
        public static uint Samples = 512;
        public static uint Bounces = 2;

        /// <summary>
        /// Same frame throttling as the editor, Preferences > General > Interaction Mode
        /// </summary>
        public static uint ThrottleMs
        {
            get
            {
                const int DefaultIdleTimeMs = 4;
                switch (EditorPrefs.GetInt("InteractionMode", 0))
                {
                    case 1: // No Throttling
                        return 0;
                    case 2: // Monitor Refresh Rate
                        {
                            double hz = Screen.currentResolution.refreshRateRatio.value;
                            return hz > 0 ? (uint)Math.Round(1000.0 / hz) : DefaultIdleTimeMs;
                        }
                    case 3: // Custom
                        return (uint)Mathf.Max(0, EditorPrefs.GetInt("ApplicationIdleTime", DefaultIdleTimeMs));
                    default:
                        return DefaultIdleTimeMs;
                }
            }
        }

        public static float Scale
        {
            get
            {
                float scale = EditorPrefs.GetFloat(ScalePrefKey, 1f);
                return scale > 0f ? scale : 1f;
            }
            set => EditorPrefs.SetFloat(ScalePrefKey, value);
        }

        static bool _active;
        static SceneView _sceneView;

        static Texture2D _texture;
        static Material _material;
        static Mesh _quad;

        static Bindings.PreviewStats _stats;

        static Vector2Int _size;
        static Vector2Int _pendingSize;
        static double _pendingSince;

        internal static Bindings.PreviewStats Stats => _stats;
        internal static bool HasFrame => _texture;

        /// <summary>
        /// The scene view showing the preview, null when it isn't running
        /// </summary>
        public static SceneView Target => _active && Bake.IsPreviewing ? _sceneView : null;

        public static void Begin()
        {
            if (_active)
            {
                return;
            }

            _active = true;
            _sceneView = SceneView.lastActiveSceneView;
            _size = Vector2Int.zero;
            _pendingSize = Vector2Int.zero;
            _stats = default;

            EditorApplication.update += Tick;
            SceneView.beforeSceneGui += BeforeSceneGui;
            AssemblyReloadEvents.beforeAssemblyReload += Cleanup;

            ScenePreviewOverlay.UpdateAll();
            SceneView.RepaintAll();
        }

        /// <summary>
        /// Stops the preview, the bake calls <see cref="End"/> once the render thread exits
        /// </summary>
        public static void Stop() => Bake.StopPreview();

        /// <summary>
        /// Called by the bake when the preview finished
        /// </summary>
        public static void End()
        {
            if (!_active)
            {
                return;
            }

            Cleanup();
            ScenePreviewOverlay.UpdateAll();
            SceneView.RepaintAll();
        }

        static void Cleanup()
        {
            _active = false;
            _sceneView = null;

            EditorApplication.update -= Tick;
            SceneView.beforeSceneGui -= BeforeSceneGui;
            AssemblyReloadEvents.beforeAssemblyReload -= Cleanup;

            if (_texture)
            {
                UnityEngine.Object.DestroyImmediate(_texture);
                _texture = null;
            }
            if (_material)
            {
                UnityEngine.Object.DestroyImmediate(_material);
                _material = null;
            }
            if (_quad)
            {
                UnityEngine.Object.DestroyImmediate(_quad);
                _quad = null;
            }
        }

        static void Tick()
        {
            if (!Bake.IsPreviewing)
            {
                return;
            }

            // follows the scene view that was used last
            var last = SceneView.lastActiveSceneView;
            if (last && last != _sceneView)
            {
                var previous = _sceneView;
                _sceneView = last;
                _size = Vector2Int.zero;
                if (previous)
                {
                    previous.Repaint();
                }
                ScenePreviewOverlay.UpdateAll();
            }

            if (!_sceneView || !_sceneView.camera)
            {
                // every scene view was closed
                Stop();
                return;
            }

            var camera = _sceneView.camera;

            Bindings.app_preview_set_settings(new Bindings.PreviewSettings
            {
                sample_count = Samples,
                bounce_count = Bounces,
                throttle_ms = ThrottleMs,
            });

            var transform = camera.transform;
            Bindings.app_preview_set_camera(new Bindings.PreviewCamera
            {
                position = transform.position,
                forward = transform.forward,
                up = transform.up,
                fov = camera.fieldOfView,
                ortho_size = camera.orthographic ? camera.orthographicSize : 0f,
            });

            UpdateSize(camera);

            Bindings.app_preview_get_stats(out _stats);

            if (Bindings.app_preview_acquire_frame(out var frame))
            {
                try
                {
                    CopyFrame(frame);
                }
                finally
                {
                    Bindings.app_preview_release_frame();
                }

                _texture.Apply(false, false);
                _sceneView.Repaint();
            }
        }

        static void UpdateSize(Camera camera)
        {
            // pixelWidth is already in pixels, the scene view camera matches its render target
            float scale = Scale;
            var size = new Vector2Int(
                Mathf.Max(1, Mathf.RoundToInt(camera.pixelWidth * scale)),
                Mathf.Max(1, Mathf.RoundToInt(camera.pixelHeight * scale)));

            if (size != _pendingSize)
            {
                _pendingSize = size;
                _pendingSince = EditorApplication.timeSinceStartup;
            }

            if (_size == Vector2Int.zero)
            {
                _size = size;
            }

            // debounce so dragging the window edge doesn't restart the render every frame
            if (_pendingSize != _size && EditorApplication.timeSinceStartup - _pendingSince > ResizeDelay)
            {
                _size = _pendingSize;
            }

            // sent every tick since the render thread resets it when the preview (re)starts
            Bindings.app_preview_resize((uint)_size.x, (uint)_size.y);
        }

        static unsafe void CopyFrame(Bindings.PreviewFrame frame)
        {
            int width = (int)frame.width;
            int height = (int)frame.height;

            if (!_texture || _texture.width != width || _texture.height != height)
            {
                if (_texture)
                {
                    UnityEngine.Object.DestroyImmediate(_texture);
                }

                _texture = new Texture2D(width, height, TextureFormat.RGBAHalf, false, true)
                {
                    name = "Glim Preview",
                    hideFlags = HideFlags.HideAndDontSave,
                    wrapMode = TextureWrapMode.Clamp,
                    filterMode = FilterMode.Bilinear,
                };
            }

            var data = _texture.GetRawTextureData<byte>();
            long bytes = (long)width * height * 8;
            Debug.Assert(data.Length >= bytes);

            UnsafeUtility.MemCpy(
                Unity.Collections.LowLevel.Unsafe.NativeArrayUnsafeUtility.GetUnsafePtr(data),
                (void*)frame.pixels,
                bytes);
        }

        static void BeforeSceneGui(SceneView sceneView)
        {
            if (Event.current.type != EventType.Repaint || sceneView != Target || !_texture)
            {
                return;
            }

            if (!EnsureDrawResources())
            {
                return;
            }

            _material.mainTexture = _texture;

            // queued for this camera's next render only, which happens right after this callback
            Graphics.DrawMesh(_quad, Matrix4x4.identity, _material, 0, sceneView.camera, 0, null, false, false, false);
        }

        static bool EnsureDrawResources()
        {
            if (!_material)
            {
                var shader = AssetDatabase.LoadAssetAtPath<Shader>(ShaderPath);
                if (!shader)
                {
                    return false;
                }

                _material = new Material(shader)
                {
                    name = "Glim Scene Preview",
                    hideFlags = HideFlags.HideAndDontSave,
                };
            }

            if (!_quad)
            {
                // corners in clip space, the shader ignores the transform
                _quad = new Mesh
                {
                    name = "Glim Scene Preview Quad",
                    hideFlags = HideFlags.HideAndDontSave,
                    vertices = new[]
                    {
                        new Vector3(-1f, -1f, 0f),
                        new Vector3(1f, -1f, 0f),
                        new Vector3(1f, 1f, 0f),
                        new Vector3(-1f, 1f, 0f),
                    },
                    triangles = new[] { 0, 2, 1, 0, 3, 2 },
                };
                // never culled, it always covers the whole view
                _quad.bounds = new Bounds(Vector3.zero, Vector3.one * 1e7f);
            }

            return true;
        }
    }

    /// <summary>
    /// Stats and preview settings, shown in the scene view that displays the preview while it's running
    /// </summary>
#if UNITY_6000_0_OR_NEWER
    [Overlay(typeof(SceneView), Id, "Glim Preview", false, defaultDockZone = DockZone.Floating, defaultDockPosition = DockPosition.Top)]
#else
    [Overlay(typeof(SceneView), Id, "Glim Preview", false)]
#endif
    class ScenePreviewOverlay : IMGUIOverlay
    {
        const string Id = "glim-scene-preview";

        static readonly List<ScenePreviewOverlay> Instances = new();

        bool? _shown;

        public override void OnCreated()
        {
            Instances.Add(this);
            Update();
        }

        public override void OnWillBeDestroyed()
        {
            Instances.Remove(this);
        }

        internal static void UpdateAll()
        {
            foreach (var overlay in Instances)
            {
                overlay.Update();
            }
        }

        void Update()
        {
            bool show = ScenePreview.Target && containerWindow == ScenePreview.Target;
            // only on change, so it doesn't fight the user hiding it from the overlay menu
            if (_shown != show)
            {
                _shown = show;
                displayed = show;
            }
        }

        // fontSize 0 means the skin default
        static int Larger(GUIStyle style, int fallback) => (style.fontSize > 0 ? style.fontSize : fallback) + 1;

        static GUIStyle _label;
        static GUIStyle Label => _label ??= new GUIStyle(EditorStyles.miniLabel)
        {
            fontSize = Larger(EditorStyles.miniLabel, 9),
        };

        static GUIStyle _field;
        static GUIStyle Field => _field ??= new GUIStyle(EditorStyles.toolbarTextField)
        {
            fontSize = Larger(EditorStyles.toolbarTextField, 9),
        };

        static GUIStyle _popup;
        static GUIStyle Popup => _popup ??= new GUIStyle(EditorStyles.toolbarPopup)
        {
            fontSize = Larger(EditorStyles.toolbarPopup, 9),
        };

        public override void OnGUI()
        {
            if (!ScenePreview.Target)
            {
                GUILayout.Label("Not running", Label);
                return;
            }

            var stats = ScenePreview.Stats;

            using (new GUILayout.HorizontalScope())
            {
                if (ScenePreview.HasFrame)
                {
                    GUILayout.Label(
                        // sample last so changing digits don't shift the other stats
                        $"{stats.width}x{stats.height}  |  Lights {stats.light_count}  |  Hardware RT: {stats.hardware_rt}  |  Sample {stats.sample_index}/{stats.sample_count}",
                        Label);
                }
                else
                {
                    GUILayout.Label("Starting preview...", Label);
                }

                GUILayout.Space(8);

                GUILayout.Label(new GUIContent("Samples", "Preview sample count, restarts the preview"), Label);
                ScenePreview.Samples = (uint)Mathf.Clamp(EditorGUILayout.DelayedIntField((int)ScenePreview.Samples, Field, GUILayout.Width(50)), 1, 65536);

                GUILayout.Label(new GUIContent("Bounces", "Preview bounce count, restarts the preview"), Label);
                ScenePreview.Bounces = (uint)Mathf.Clamp(EditorGUILayout.DelayedIntField((int)ScenePreview.Bounces, Field, GUILayout.Width(30)), 0, 127);

                GUILayout.Label(new GUIContent("Resolution", "Render resolution relative to the scene view size"), Label);
                int current = Array.IndexOf(ScenePreview.Scales, ScenePreview.Scale);
                if (current < 0) current = 0;
                int selected = EditorGUILayout.Popup(current, ScenePreview.ScaleNames, Popup, GUILayout.Width(60));
                if (selected != current)
                {
                    ScenePreview.Scale = ScenePreview.Scales[selected];
                }

                if (GUILayout.Button(new GUIContent("Stop", "Stops the preview"), EditorStyles.toolbarButton, GUILayout.Width(40)))
                {
                    ScenePreview.Stop();
                }
            }
        }
    }
}
