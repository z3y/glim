use ash::vk;

use crate::vulkan_core::VulkanContext;

impl VulkanContext {
    pub fn begin_temp_graphics_cmd(self: &Self) -> vk::CommandBuffer {
        let allocate_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.graphics_command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);

        let cmd = unsafe { self.device.allocate_command_buffers(&allocate_info) }.unwrap()[0];

        let begin_info = vk::CommandBufferBeginInfo {
            flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            ..Default::default()
        };

        unsafe { self.device.begin_command_buffer(cmd, &begin_info) }.unwrap();

        println!("aa");
        cmd
    }

    pub fn end_temp_graphics_cmd(self: &Self, cmd: vk::CommandBuffer) {
        unsafe { self.device.end_command_buffer(cmd) }.unwrap();

        let cmds = [cmd];
        let submit = vk::SubmitInfo::default().command_buffers(&cmds);

        unsafe {
            self.device
                .queue_submit(self.graphics_queue, &[submit], vk::Fence::null())
                .unwrap();
        };

        unsafe { self.device.queue_wait_idle(self.graphics_queue).unwrap() };

        unsafe {
            self.device
                .free_command_buffers(self.graphics_command_pool, &cmds)
        };
    }
}
