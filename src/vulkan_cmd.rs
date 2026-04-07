use ash::vk;

use crate::vulkan_core::VulkanObjects;

pub fn begin_temp_graphics_cmd(vk: &VulkanObjects) -> vk::CommandBuffer {
    let allocate_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(vk.graphics_command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);

    let cmd = unsafe { vk.device.allocate_command_buffers(&allocate_info) }.unwrap()[0];

    let begin_info = vk::CommandBufferBeginInfo {
        flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
        ..Default::default()
    };

    unsafe { vk.device.begin_command_buffer(cmd, &begin_info) }.unwrap();

    println!("aa");
    cmd
}

pub fn end_temp_graphics_cmd(vk: &VulkanObjects, cmd: vk::CommandBuffer) {
    unsafe { vk.device.end_command_buffer(cmd) }.unwrap();

    let cmds = [cmd];
    let submit = vk::SubmitInfo::default().command_buffers(&cmds);

    unsafe {
        vk.device
            .queue_submit(vk.graphics_queue, &[submit], vk::Fence::null())
            .unwrap();
    };

    unsafe { vk.device.queue_wait_idle(vk.graphics_queue).unwrap() };

    unsafe {
        vk.device
            .free_command_buffers(vk.graphics_command_pool, &cmds)
    };
}
