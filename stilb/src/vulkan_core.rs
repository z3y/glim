use ash::{
    Device, Entry, Instance, khr,
    vk::{self, DebugUtilsMessageSeverityFlagsEXT, DebugUtilsMessageTypeFlagsEXT, Handle},
};

use std::{
    collections::HashSet,
    ffi::{CStr, c_char},
    ptr,
};

use crate::vulkan_swapchain::SwapchainData;

pub struct VulkanConfig {
    pub enable_validation_layers: bool,
    pub enable_window: bool,
    pub window_extensions: Vec<*const c_char>,
}

#[derive(Debug)]
pub struct QueueFamilyIndices {
    pub graphics: u32,
    pub compute: u32,
    pub present: u32,
}

pub struct VulkanContext {
    pub entry: Entry,

    pub instance: Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,

    pub surface_instance: khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub swapchain_device: khr::swapchain::Device,

    pub queue_family_indices: QueueFamilyIndices,
    pub graphics_queue: vk::Queue,
    pub compute_queue: vk::Queue,
    pub present_queue: vk::Queue,

    pub command_pool: vk::CommandPool,

    pub command_buffer: vk::CommandBuffer,

    pub descriptor_pool: vk::DescriptorPool,

    pub swapchain: SwapchainData,

    pub as_device: Option<khr::acceleration_structure::Device>,
}

impl VulkanContext {
    pub fn new(
        config: &VulkanConfig,
        create_surface_callback: impl Fn(&Instance) -> vk::SurfaceKHR,
    ) -> Self {
        let entry = ash::Entry::linked();

        let app_name = c"stilb";
        let validation_layer_name = c"VK_LAYER_KHRONOS_validation";

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

        for ext in &config.window_extensions {
            extensions.push(*ext);
            // let str = unsafe { CStr::from_ptr(*ext) };
            // println!("Adding: {:?}", str);
        }

        println!("Validation Layers: {} ", config.enable_validation_layers);

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

        let instance = unsafe { entry.create_instance(&create_info, None).unwrap() };

        let surface_instance = khr::surface::Instance::new(&entry, &instance);

        let surface = if config.enable_window {
            create_surface_callback(&instance)
        } else {
            vk::SurfaceKHR::null()
        };

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
        } else {
            println!("VK_KHR_ray_query supported");
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

        let graphics_queue = unsafe { device.get_device_queue(queue_family_indices.graphics, 0) };
        let compute_queue = unsafe { device.get_device_queue(queue_family_indices.compute, 0) };
        let present_queue = unsafe { device.get_device_queue(queue_family_indices.present, 0) };

        // for now only this is supported
        assert_eq!(compute_queue, graphics_queue);

        if config.enable_window {
            // create_swapchain();
        }

        let pool_info = vk::CommandPoolCreateInfo {
            flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            queue_family_index: queue_family_indices.compute,
            ..Default::default()
        };
        let command_pool = unsafe { device.create_command_pool(&pool_info, None) }.unwrap();

        let mut pool_sizes = Vec::new();
        let storage_image_pool = vk::DescriptorPoolSize {
            descriptor_count: 3,
            ty: vk::DescriptorType::STORAGE_IMAGE,
        };
        let sampled_image_pool = vk::DescriptorPoolSize {
            descriptor_count: 2,
            ty: vk::DescriptorType::SAMPLED_IMAGE,
        };
        let storage_buffer_pool = vk::DescriptorPoolSize {
            descriptor_count: 5,
            ty: vk::DescriptorType::STORAGE_BUFFER,
        };
        let as_structure_pool = vk::DescriptorPoolSize {
            descriptor_count: 1,
            ty: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
        };

        pool_sizes.push(storage_image_pool);
        pool_sizes.push(sampled_image_pool);
        pool_sizes.push(storage_buffer_pool);
        if has_ray_query {
            pool_sizes.push(as_structure_pool);
        }

        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::default()
            .pool_sizes(&mut pool_sizes)
            .max_sets(1);

        let descriptor_pool = unsafe { device.create_descriptor_pool(&descriptor_pool_info, None) }
            .expect("failed to create descriptor pool");

        // todo: semaphores and fences

        let as_device = if has_ray_query {
            Some(khr::acceleration_structure::Device::new(&instance, &device))
        } else {
            None
        };

        let swapchain_device = khr::swapchain::Device::new(&instance, &device);

        let swapchain = SwapchainData {
            swapchain: vk::SwapchainKHR::null(),
            frames: Vec::new(),
        };

        let allocate_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);

        let command_buffer = unsafe { device.allocate_command_buffers(&allocate_info) }.unwrap()[0];

        Self {
            entry,
            instance,
            physical_device,
            device,
            queue_family_indices,
            graphics_queue,
            compute_queue,
            present_queue,
            command_pool,
            descriptor_pool,
            surface,
            surface_instance,
            as_device,
            swapchain_device,
            swapchain,
            command_buffer,
        }
    }

    pub fn find_memory_type(
        self: &Self,
        type_filter: u32,
        properties: vk::MemoryPropertyFlags,
    ) -> u32 {
        let memory_properties = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical_device)
        };

        for i in 0..memory_properties.memory_type_count {
            let is_type_supported = (type_filter & (1 << i)) != 0;

            let has_required_properties = memory_properties.memory_types[i as usize]
                .property_flags
                .contains(properties);

            if is_type_supported && has_required_properties {
                return i;
            }
        }

        panic!("Failed to find a suitable memory type");
    }

    pub fn create_buffer(
        &self,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        properties: vk::MemoryPropertyFlags,
    ) -> (vk::Buffer, vk::DeviceMemory) {
        let create_info = vk::BufferCreateInfo {
            size,
            usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };

        let buffer = unsafe { self.device.create_buffer(&create_info, None).unwrap() };

        let mem_reqs = unsafe { self.device.get_buffer_memory_requirements(buffer) };

        let memory_type_index = self.find_memory_type(mem_reqs.memory_type_bits, properties);

        let mut allocate_info = vk::MemoryAllocateInfo {
            allocation_size: mem_reqs.size,
            memory_type_index,
            ..Default::default()
        };

        let mut allocate_flags = vk::MemoryAllocateFlagsInfo {
            flags: vk::MemoryAllocateFlags::DEVICE_ADDRESS,
            ..Default::default()
        };

        allocate_info = allocate_info.push_next(&mut allocate_flags);

        let memory = unsafe { self.device.allocate_memory(&allocate_info, None) }.unwrap();

        unsafe { self.device.bind_buffer_memory(buffer, memory, 0) }.unwrap();

        (buffer, memory)
    }

    pub fn upload_buffer(&self, src: &[u8], dst: vk::Buffer) {
        let size = src.len() as vk::DeviceSize;

        let usage = vk::BufferUsageFlags::TRANSFER_SRC;
        let properties =
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;

        let (staging_buffer, staging_memory) = self.create_buffer(size, usage, properties);

        let ptr = unsafe {
            self.device
                .map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty())
                .unwrap()
        } as *mut u8;

        unsafe {
            ptr::copy_nonoverlapping(src.as_ptr(), ptr, src.len());
            // self.device.unmap_memory(staging_memory);
        };

        let cmd = self.begin_single_use_cmd();

        let regions = vk::BufferCopy {
            src_offset: 0,
            dst_offset: 0,
            size,
        };

        unsafe {
            self.device
                .cmd_copy_buffer(cmd, staging_buffer, dst, &[regions])
        };

        self.end_single_use_cmd(cmd);

        unsafe {
            self.device.destroy_buffer(staging_buffer, None);
            self.device.free_memory(staging_memory, None);
        };
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

fn find_queue_families(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> QueueFamilyIndices {
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

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            self.device
                .device_wait_idle()
                .expect("Failed to wait for device idle");

            self.destroy_swapchain();

            let device = &self.device;
            let instance = &self.instance;

            device.destroy_descriptor_pool(self.descriptor_pool, None);

            device.destroy_command_pool(self.command_pool, None);

            if !self.surface.is_null() {
                self.surface_instance.destroy_surface(self.surface, None);
            }

            device.destroy_device(None);

            instance.destroy_instance(None);
        }
    }
}
