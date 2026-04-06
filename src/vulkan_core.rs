use ash::{
    Device, Entry, Instance,
    vk::{self, DebugUtilsMessageSeverityFlagsEXT, DebugUtilsMessageTypeFlagsEXT, PhysicalDevice},
};
use std::{
    collections::HashSet,
    ffi::{CStr, CString},
};

pub struct VulkanConfig {
    pub enable_validation_layers: bool,
    pub enable_window: bool,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct QueueFamilyIndices {
    pub graphics: u32,
    pub compute: u32,
    pub present: u32,
}

pub struct VulkanObjects {
    pub entry: Entry,
    pub instance: Instance,
    pub physical_device: PhysicalDevice,
    pub device: Device,
    pub queue_family_indices: QueueFamilyIndices,
}

impl Drop for VulkanObjects {
    fn drop(&mut self) {
        unsafe {
            self.device
                .device_wait_idle()
                .expect("Failed to wait for device idle");

            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

pub extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    unsafe {
        let callback_data = *p_callback_data;
        let message = CStr::from_ptr(callback_data.p_message).to_string_lossy();
        println!("{message_severity:?}: {message_type:?} | {message}");
    }

    vk::FALSE
}

fn find_queue_families(instance: &Instance, physical_device: PhysicalDevice) -> QueueFamilyIndices {
    let mut graphics = None;
    let mut compute = None;
    let mut present = None;

    let properties =
        unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

    for (i, prop) in properties.iter().enumerate() {
        if prop.queue_flags.contains(vk::QueueFlags::COMPUTE) {
            compute = Some(i as u32);
        }

        if prop.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            graphics = Some(i as u32);

            // todo: find present queue
            present = Some(i as u32);
        }

        if graphics.is_some() && compute.is_some() && present.is_some() {
            break;
        }
    }

    QueueFamilyIndices {
        graphics: graphics.expect("Failed to find graphics queue"),
        compute: compute.expect("Failed to find compute queue"),
        present: present.expect("Failed to find present queue"),
    }
}

pub fn vulkan_initialize(config: &VulkanConfig) -> VulkanObjects {
    let app_name = CString::new("stilb").unwrap();
    let validation_layer_name = CString::new("VK_LAYER_KHRONOS_validation").unwrap();

    let application_info = vk::ApplicationInfo {
        p_application_name: app_name.as_ptr(),
        application_version: 1,
        engine_version: 1,
        api_version: vk::API_VERSION_1_3,
        ..Default::default()
    };

    let mut extensions = Vec::new();
    let mut layers = Vec::new();

    if config.enable_validation_layers {
        extensions.push(vk::EXT_DEBUG_UTILS_NAME.as_ptr());
        layers.push(validation_layer_name.as_ptr());
    }

    let mut create_info = vk::InstanceCreateInfo {
        p_application_info: &application_info,
        enabled_layer_count: layers.len() as u32,
        pp_enabled_layer_names: layers.as_ptr(),
        enabled_extension_count: extensions.len() as u32,
        pp_enabled_extension_names: extensions.as_ptr(),
        ..Default::default()
    };

    let mut debug_create_info = vk::DebugUtilsMessengerCreateInfoEXT {
        message_severity: DebugUtilsMessageSeverityFlagsEXT::ERROR
            | DebugUtilsMessageSeverityFlagsEXT::WARNING,
        message_type: DebugUtilsMessageTypeFlagsEXT::GENERAL
            | DebugUtilsMessageTypeFlagsEXT::VALIDATION
            | DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        pfn_user_callback: Some(vulkan_debug_callback),
        ..Default::default()
    };

    if config.enable_validation_layers {
        create_info = create_info.push_next(&mut debug_create_info);
    }

    let entry = unsafe { ash::Entry::load().unwrap() };
    let instance = unsafe { entry.create_instance(&create_info, None).unwrap() };

    let physical_devices = unsafe { instance.enumerate_physical_devices().unwrap() };
    for physical_device in &physical_devices {
        let properties = unsafe { instance.get_physical_device_properties(*physical_device) };

        let name = properties.device_name_as_c_str();
        println!("Device {:?}", name);
    }
    // TODO: find the right device
    let physical_device = physical_devices[0];

    let queue_family_indices = find_queue_families(&instance, physical_device);
    println!("{:?}", queue_family_indices);

    let mut unique_queues = HashSet::new();
    unique_queues.insert(queue_family_indices.compute);
    unique_queues.insert(queue_family_indices.graphics);
    unique_queues.insert(queue_family_indices.present);
    println!("unique_queues: {:?}", unique_queues.len());

    let mut queue_create_infos = Vec::new();
    let queue_priority = 1.0;
    for queue in unique_queues {
        let info = vk::DeviceQueueCreateInfo {
            queue_family_index: queue,
            queue_count: 1,
            p_queue_priorities: &queue_priority,
            ..Default::default()
        };
        queue_create_infos.push(info);
    }

    let mut device_extensions = Vec::new();

    if config.enable_window {
        device_extensions.push(vk::KHR_SWAPCHAIN_NAME.as_ptr());
    }

    device_extensions.push(vk::KHR_BUFFER_DEVICE_ADDRESS_NAME.as_ptr());

    let avalilable_extensions = unsafe {
        instance
            .enumerate_device_extension_properties(physical_device)
            .unwrap()
    };

    let mut has_ray_query = false;
    for ext in avalilable_extensions {
        if ext.extension_name_as_c_str().unwrap() == vk::KHR_RAY_QUERY_NAME {
            has_ray_query = true;
        }
    }

    if !has_ray_query {
        println!("VK_KHR_ray_query not supported");
    }

    if has_ray_query {
        device_extensions.push(vk::KHR_ACCELERATION_STRUCTURE_NAME.as_ptr());
        device_extensions.push(vk::KHR_DEFERRED_HOST_OPERATIONS_NAME.as_ptr());
        device_extensions.push(vk::KHR_RAY_QUERY_NAME.as_ptr());
    }

    let device_features = vk::PhysicalDeviceFeatures {
        geometry_shader: vk::TRUE,
        ..Default::default()
    };

    let mut features12 = vk::PhysicalDeviceVulkan12Features {
        buffer_device_address: vk::TRUE,
        ..Default::default()
    };

    let mut device_features2 = vk::PhysicalDeviceFeatures2 {
        features: device_features,
        ..Default::default()
    };

    let mut device_create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_extension_names(&device_extensions)
        .push_next(&mut device_features2)
        .push_next(&mut features12);

    let mut ray_query_features = vk::PhysicalDeviceRayQueryFeaturesKHR {
        ray_query: vk::TRUE,
        ..Default::default()
    };
    let mut as_features = vk::PhysicalDeviceAccelerationStructureFeaturesKHR {
        acceleration_structure: vk::TRUE,
        ..Default::default()
    };
    if has_ray_query {
        device_create_info = device_create_info
            .push_next(&mut as_features)
            .push_next(&mut ray_query_features);
    }

    let device = unsafe {
        instance
            .create_device(physical_device, &device_create_info, None)
            .unwrap()
    };

    return VulkanObjects {
        entry,
        instance,
        physical_device,
        device,
        queue_family_indices,
    };
}
