using System;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace Glim
{
    /// <summary>
    /// Vulkan device used for baking and the preview. Stored per machine in EditorPrefs by name,
    /// an empty name picks the best device automatically.
    /// </summary>
    public static class VulkanDevice
    {
        const string PrefKey = "Glim.VulkanDevice";

        static string[] _devices;

        public static string SelectedName
        {
            get => EditorPrefs.GetString(PrefKey, string.Empty);
            set => EditorPrefs.SetString(PrefKey, value ?? string.Empty);
        }

        /// <summary>
        /// Device names in the order glim enumerates them, cached until <see cref="Refresh"/>
        /// </summary>
        public static string[] Devices => _devices ??= Enumerate();

        public static void Refresh() => _devices = null;

        /// <summary>
        /// Index passed to glim, -1 for automatic or when the saved device isn't present
        /// </summary>
        public static int SelectedIndex
        {
            get
            {
                string name = SelectedName;
                if (string.IsNullOrEmpty(name))
                {
                    return -1;
                }

                int index = Array.IndexOf(Devices, name);
                if (index < 0)
                {
                    Debug.LogWarning($"Glim: Vulkan device \"{name}\" not found, selecting a device automatically");
                }
                return index;
            }
        }

        static unsafe string[] Enumerate()
        {
            try
            {
                uint size = Bindings.app_list_vulkan_devices(null, 0);
                if (size == 0)
                {
                    return Array.Empty<string>();
                }

                var buffer = new byte[size];
                fixed (byte* ptr = buffer)
                {
                    Bindings.app_list_vulkan_devices(ptr, size);
                }

                return Encoding.UTF8.GetString(buffer).Split('\n');
            }
            catch (Exception e)
            {
                Debug.LogException(e);
                return Array.Empty<string>();
            }
        }
    }
}
