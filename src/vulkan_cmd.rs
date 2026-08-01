use ash::vk;

use crate::vulkan_context::VulkanContext;

impl VulkanContext {
    pub fn begin_single_use_cmd(self: &Self) -> vk::CommandBuffer {
        let cmd = self.command_buffer;

        let begin_info = vk::CommandBufferBeginInfo {
            flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            ..Default::default()
        };

        unsafe {
            self.device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                .unwrap();

            self.device.begin_command_buffer(cmd, &begin_info)
        }
        .unwrap();

        cmd
    }

    pub fn end_single_use_cmd(self: &Self, cmd: vk::CommandBuffer) {
        let cmds = [cmd];
        let submit = vk::SubmitInfo::default().command_buffers(&cmds);

        unsafe {
            self.device.end_command_buffer(cmd).unwrap();

            self.device
                .queue_submit(self.compute_queue, &[submit], vk::Fence::null())
                .unwrap();

            self.device.queue_wait_idle(self.compute_queue).unwrap()
        };
    }
}
