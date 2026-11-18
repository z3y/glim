using System;
using Unity.Collections.LowLevel.Unsafe;
using UnityEditor;
using UnityEngine;

namespace Glim
{
    /// <summary>
    /// Displays the path traced preview. Frames are rendered by glim on its own Vulkan device and read back
    /// into CPU memory, then uploaded into a regular Texture2D, so it works with any editor graphics API.
    /// </summary>
    public class PreviewWindow : EditorWindow
    {
        const float HeaderHeight = 20f;
        const double ResizeDelay = 0.15;
        const string ScalePrefKey = "Glim.PreviewWindow.Scale";

        static readonly float[] Scales = { 1f, 0.75f, 0.5f, 0.25f };
        static readonly string[] ScaleNames = { "100%", "75%", "50%", "25%" };

        // only the first frame, the window size takes over right after
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

        Texture2D _texture;
        Bindings.PreviewStats _stats;

        Bindings.PreviewKeys _keys;
        bool _looking;
        Vector2 _lookDelta;
        int _lookControlId;

        Vector2Int _size;
        Vector2Int _pendingSize;
        double _pendingSince;

        bool _closedByBake;

        public static void Open()
        {
            var window = GetWindow<PreviewWindow>();
            window.titleContent = new GUIContent("Glim Preview", EditorGUIUtility.IconContent("animationvisibilitytoggleon").image);
            window.Show();
            window.Focus();
        }

        public static void CloseFromBake()
        {
            foreach (var window in Resources.FindObjectsOfTypeAll<PreviewWindow>())
            {
                window._closedByBake = true;
                window.Close();
            }
        }

        void OnEnable()
        {
            EditorApplication.update += Tick;
        }

        void OnDisable()
        {
            EditorApplication.update -= Tick;
            StopLooking();

            if (_texture)
            {
                DestroyImmediate(_texture);
                _texture = null;
            }
        }

        void OnDestroy()
        {
            if (!_closedByBake)
            {
                Bake.StopPreview();
            }
        }

        void OnLostFocus()
        {
            _keys = Bindings.PreviewKeys.None;
            StopLooking();
        }

        float Scale
        {
            get
            {
                float scale = EditorPrefs.GetFloat(ScalePrefKey, 1f);
                return scale > 0f ? scale : 1f;
            }
            set => EditorPrefs.SetFloat(ScalePrefKey, value);
        }

        void Tick()
        {
            if (!Bake.IsPreviewing)
            {
                return;
            }

            Bindings.app_preview_set_input(new Bindings.PreviewInput
            {
                keys = _keys,
                look_delta_x = _lookDelta.x,
                look_delta_y = _lookDelta.y,
                looking = _looking,
            });
            _lookDelta = Vector2.zero;

            Bindings.app_preview_set_settings(new Bindings.PreviewSettings
            {
                sample_count = Samples,
                bounce_count = Bounces,
                throttle_ms = ThrottleMs,
            });

            // debounce so dragging the window edge doesn't restart the render every frame
            if (_pendingSize != _size && EditorApplication.timeSinceStartup - _pendingSince > ResizeDelay)
            {
                _size = _pendingSize;
            }

            // sent every tick since the render thread resets it when the preview (re)starts
            if (_size.x > 0 && _size.y > 0)
            {
                Bindings.app_preview_resize((uint)_size.x, (uint)_size.y);
            }

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
                Repaint();
            }
        }

        unsafe void CopyFrame(Bindings.PreviewFrame frame)
        {
            int width = (int)frame.width;
            int height = (int)frame.height;

            if (!_texture || _texture.width != width || _texture.height != height)
            {
                if (_texture)
                {
                    DestroyImmediate(_texture);
                }

                // linear: false, the pixels are sRGB encoded
                _texture = new Texture2D(width, height, TextureFormat.RGBA32, false, false)
                {
                    name = "Glim Preview",
                    hideFlags = HideFlags.HideAndDontSave,
                    wrapMode = TextureWrapMode.Clamp,
                    filterMode = FilterMode.Bilinear,
                };
            }

            var data = _texture.GetRawTextureData<byte>();
            long bytes = (long)width * height * 4;
            Debug.Assert(data.Length >= bytes);

            UnsafeUtility.MemCpy(
                Unity.Collections.LowLevel.Unsafe.NativeArrayUnsafeUtility.GetUnsafePtr(data),
                (void*)frame.pixels,
                bytes);
        }

        void OnGUI()
        {
            _lookControlId = GUIUtility.GetControlID(FocusType.Passive);

            DrawHeader();

            var imageRect = new Rect(0, HeaderHeight, position.width, Mathf.Max(0, position.height - HeaderHeight));

            if (Event.current.type == EventType.Repaint)
            {
                float scale = EditorGUIUtility.pixelsPerPoint * Scale;
                var size = new Vector2Int(
                    Mathf.Max(1, Mathf.RoundToInt(imageRect.width * scale)),
                    Mathf.Max(1, Mathf.RoundToInt(imageRect.height * scale)));

                if (size != _pendingSize)
                {
                    _pendingSize = size;
                    _pendingSince = EditorApplication.timeSinceStartup;
                }

                if (_size == Vector2Int.zero)
                {
                    _size = size;
                }

                EditorGUI.DrawRect(imageRect, Color.black);
                if (_texture)
                {
                    // vulkan rows start at the top, unity textures at the bottom
                    GUI.DrawTextureWithTexCoords(imageRect, _texture, new Rect(0, 1, 1, -1), false);
                }
                else
                {
                    GUI.Label(imageRect, Bake.IsPreviewing ? "Starting preview..." : "Preview not running", CenteredLabel);
                }
            }

            HandleInput(imageRect);
        }

        static GUIStyle _centeredLabel;
        // fontSize 0 means the skin default
        static int Larger(GUIStyle style, int fallback) => (style.fontSize > 0 ? style.fontSize : fallback) + 1;

        static GUIStyle _headerLabel;
        static GUIStyle HeaderLabel => _headerLabel ??= new GUIStyle(EditorStyles.miniLabel)
        {
            fontSize = Larger(EditorStyles.miniLabel, 9),
        };

        static GUIStyle _headerField;
        static GUIStyle HeaderField => _headerField ??= new GUIStyle(EditorStyles.toolbarTextField)
        {
            fontSize = Larger(EditorStyles.toolbarTextField, 9),
        };

        static GUIStyle _headerPopup;
        static GUIStyle HeaderPopup => _headerPopup ??= new GUIStyle(EditorStyles.toolbarPopup)
        {
            fontSize = Larger(EditorStyles.toolbarPopup, 9),
        };

        static GUIStyle CenteredLabel => _centeredLabel ??= new GUIStyle(EditorStyles.label)
        {
            fontSize = Larger(EditorStyles.label, 12),
            alignment = TextAnchor.MiddleCenter,
            normal = { textColor = Color.gray },
        };

        void DrawHeader()
        {
            using (new GUILayout.HorizontalScope(EditorStyles.toolbar, GUILayout.Height(HeaderHeight)))
            {
                if (Bake.IsPreviewing)
                {
                    GUILayout.Label(
                        // sample last so changing digits don't shift the other stats
                        $"{_stats.width}x{_stats.height}  |  Lights {_stats.light_count}  |  Hardware RT: {_stats.hardware_rt}  |  Sample {_stats.sample_index}/{_stats.sample_count}",
                        HeaderLabel);
                }
                else
                {
                    GUILayout.Label("Not running", HeaderLabel);
                }

                GUILayout.FlexibleSpace();

                GUILayout.Label(new GUIContent("Samples", "Preview sample count, restarts the preview"), HeaderLabel);
                Samples = (uint)Mathf.Clamp(EditorGUILayout.DelayedIntField((int)Samples, HeaderField, GUILayout.Width(50)), 1, 65536);

                GUILayout.Label(new GUIContent("Bounces", "Preview bounce count, restarts the preview"), HeaderLabel);
                Bounces = (uint)Mathf.Clamp(EditorGUILayout.DelayedIntField((int)Bounces, HeaderField, GUILayout.Width(30)), 0, 127);

                GUILayout.Label(new GUIContent("Resolution", "Render resolution relative to the window size"), HeaderLabel);
                int current = Array.IndexOf(Scales, Scale);
                if (current < 0) current = 0;
                int selected = EditorGUILayout.Popup(current, ScaleNames, HeaderPopup, GUILayout.Width(60));
                if (selected != current)
                {
                    Scale = Scales[selected];
                    Repaint();
                }

                GUILayout.Label(new GUIContent("RMB + WASDQE", "Hold right mouse to look, WASD to move, Q/E down/up, Shift faster, Ctrl slower, R restart, Esc close"), HeaderLabel);
            }
        }

        void HandleInput(Rect imageRect)
        {
            var e = Event.current;

            if (e.isKey || e.isMouse)
            {
                SetKey(Bindings.PreviewKeys.Fast, e.shift);
                SetKey(Bindings.PreviewKeys.Slow, e.control);
            }

            switch (e.type)
            {
                case EventType.MouseDown:
                    if (imageRect.Contains(e.mousePosition))
                    {
                        Focus();
                        if (e.button == 1)
                        {
                            _looking = true;
                            GUIUtility.hotControl = _lookControlId;
                            EditorGUIUtility.SetWantsMouseJumping(1);
                        }
                        e.Use();
                    }
                    break;

                case EventType.MouseDrag:
                    if (_looking)
                    {
                        _lookDelta += e.delta * EditorGUIUtility.pixelsPerPoint;
                        e.Use();
                    }
                    break;

                case EventType.MouseUp:
                    if (e.button == 1 && _looking)
                    {
                        StopLooking();
                        e.Use();
                    }
                    break;

                case EventType.KeyDown:
                    {
                        if (e.keyCode == KeyCode.Escape)
                        {
                            e.Use();
                            // OnDestroy stops the preview; deferred so the window isn't closed mid OnGUI
                            EditorApplication.delayCall += () => { if (this) Close(); };
                            break;
                        }

                        var key = MapKey(e.keyCode);
                        if (key != Bindings.PreviewKeys.None)
                        {
                            _keys |= key;
                            e.Use();
                        }
                        break;
                    }

                case EventType.KeyUp:
                    {
                        var key = MapKey(e.keyCode);
                        if (key != Bindings.PreviewKeys.None)
                        {
                            _keys &= ~key;
                            e.Use();
                        }
                        break;
                    }
            }
        }

        void StopLooking()
        {
            if (_looking)
            {
                EditorGUIUtility.SetWantsMouseJumping(0);
                if (GUIUtility.hotControl == _lookControlId)
                {
                    GUIUtility.hotControl = 0;
                }
            }
            _looking = false;
        }

        void SetKey(Bindings.PreviewKeys key, bool held)
        {
            if (held) _keys |= key;
            else _keys &= ~key;
        }

        static Bindings.PreviewKeys MapKey(KeyCode code) => code switch
        {
            KeyCode.W => Bindings.PreviewKeys.Forward,
            KeyCode.S => Bindings.PreviewKeys.Back,
            KeyCode.A => Bindings.PreviewKeys.Left,
            KeyCode.D => Bindings.PreviewKeys.Right,
            KeyCode.Q => Bindings.PreviewKeys.Down,
            KeyCode.E => Bindings.PreviewKeys.Up,
            KeyCode.LeftShift or KeyCode.RightShift => Bindings.PreviewKeys.Fast,
            KeyCode.LeftControl or KeyCode.RightControl => Bindings.PreviewKeys.Slow,
            KeyCode.R => Bindings.PreviewKeys.Restart,
            KeyCode.P => Bindings.PreviewKeys.PrintCamera,
            _ => Bindings.PreviewKeys.None,
        };
    }
}
