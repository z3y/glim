use std::time::Instant;

use crate::vulkan_core::{VulkanConfig, vulkan_initialize};

mod vulkan_core;

fn main() {
    let instant = Instant::now();

    {
        let config = VulkanConfig {
            enable_validation_layers: true,
            enable_window: false,
            width: 512,
            height: 512,
        };

        let vk = vulkan_initialize(&config);
    }

    let elapsed = instant.elapsed();

    println!("Elapsed: {:?}", elapsed);
}
