use ash::vk::{self, Handle};

use crate::{math::*, vulkan_core::VulkanContext};
use core::slice;

#[repr(C)]
pub struct RawMesh {
    pub vertices: *const Vector3,
    pub normals: *const Vector3,
    pub uvs: *const Vector2,
    pub indices: *const u32,
    pub vertices_length: u32,
    pub indices_length: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct Vertex {
    position: Vector3,
    normal: Vector3,
    uv: Vector2,
}

#[derive(Debug)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn from_raw_mesh(raw: RawMesh) -> Self {
        let vertices = unsafe { slice::from_raw_parts(raw.vertices, raw.vertices_length as usize) };
        let normals = unsafe { slice::from_raw_parts(raw.normals, raw.vertices_length as usize) };
        let uvs = unsafe { slice::from_raw_parts(raw.uvs, raw.vertices_length as usize) };
        let indices = unsafe { slice::from_raw_parts(raw.indices, raw.indices_length as usize) };

        let mut vertices_copy = Vec::with_capacity(vertices.len());
        let mut triangles_copy = Vec::with_capacity(indices.len());

        for i in 0..vertices.len() {
            let vertex = Vertex {
                position: vertices[i],
                normal: normals[i],
                uv: uvs[i],
            };

            vertices_copy.push(vertex);
        }

        triangles_copy.extend(indices);

        Self {
            vertices: vertices_copy,
            indices: triangles_copy,
        }
    }
}

pub struct VulkanBlas {
    blas: vk::AccelerationStructureKHR,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    address: vk::DeviceAddress,
}

pub enum BvhType {
    RayQuery(VulkanBlas),
    CwBvh,
}

pub struct GpuMesh {
    vertex_buffer: vk::Buffer,
    vertex_memory: vk::DeviceMemory,
    vertex_address: vk::DeviceAddress,

    index_buffer: vk::Buffer,
    index_memory: vk::DeviceMemory,
    index_address: vk::DeviceAddress,

    bvh: BvhType,
}

impl GpuMesh {
    pub fn new(vk: &VulkanContext, mesh: &Mesh) -> Self {
        let mut usage = vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;

        if vk.as_device.is_some() {
            usage |= vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR;
        }

        // vertices

        let size = (mesh.vertices.len() * std::mem::size_of::<Vertex>()) as vk::DeviceSize;
        let (vertex_buffer, vertex_memory) =
            vk.create_buffer(size, usage, vk::MemoryPropertyFlags::DEVICE_LOCAL);

        let info = vk::BufferDeviceAddressInfo {
            buffer: vertex_buffer,
            ..Default::default()
        };

        let vertex_address = unsafe { vk.device.get_buffer_device_address(&info) };

        let (_, bytes, _) = unsafe { mesh.vertices.align_to::<u8>() };
        vk.upload_buffer(bytes, vertex_buffer);

        // indices

        let size = (mesh.indices.len() * std::mem::size_of::<u32>()) as vk::DeviceSize;
        let (index_buffer, index_memory) =
            vk.create_buffer(size, usage, vk::MemoryPropertyFlags::DEVICE_LOCAL);

        let info = vk::BufferDeviceAddressInfo {
            buffer: index_buffer,
            ..Default::default()
        };

        let index_address = unsafe { vk.device.get_buffer_device_address(&info) };

        let (_, bytes, _) = unsafe { mesh.indices.align_to::<u8>() };
        vk.upload_buffer(bytes, index_buffer);

        let bvh = if vk.as_device.is_some() {
            BvhType::RayQuery(GpuMesh::create_vulkan_blas(
                vk,
                mesh,
                vertex_address,
                index_address,
            ))
        } else {
            BvhType::CwBvh // todo
        };

        Self {
            vertex_buffer,
            index_buffer,
            vertex_memory,
            index_memory,
            vertex_address,
            index_address,
            bvh,
        }
    }

    pub fn create_vulkan_blas(
        vk: &VulkanContext,
        mesh: &Mesh,
        vertex_address: vk::DeviceAddress,
        index_address: vk::DeviceAddress,
    ) -> VulkanBlas {
        let triangles = vk::AccelerationStructureGeometryTrianglesDataKHR {
            vertex_format: vk::Format::R32G32B32_SFLOAT,
            vertex_data: vk::DeviceOrHostAddressConstKHR {
                device_address: vertex_address,
            },
            vertex_stride: std::mem::size_of::<Vertex>() as u64,
            max_vertex: (mesh.vertices.len() - 1) as u32,
            index_type: vk::IndexType::UINT32,
            index_data: vk::DeviceOrHostAddressConstKHR {
                device_address: index_address,
            },
            ..Default::default()
        };

        let geometry = vk::AccelerationStructureGeometryKHR {
            geometry_type: vk::GeometryTypeKHR::TRIANGLES,
            geometry: vk::AccelerationStructureGeometryDataKHR {
                triangles: triangles,
            },
            flags: vk::GeometryFlagsKHR::OPAQUE,
            ..Default::default()
        };

        let geometries = [geometry];

        let build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(&geometries);

        let mut size_info = vk::AccelerationStructureBuildSizesInfoKHR::default();

        let max_primitive_count = (mesh.indices.len() / 3) as u32;

        let Some(as_device) = &vk.as_device else {
            unreachable!("expected as device");
        };

        unsafe {
            as_device.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &build_info,
                &[max_primitive_count],
                &mut size_info,
            )
        };

        let (blas_buffer, blas_memory) = vk.create_buffer(
            size_info.acceleration_structure_size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        let create_info = vk::AccelerationStructureCreateInfoKHR {
            buffer: blas_buffer,
            size: size_info.acceleration_structure_size,
            ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
            ..Default::default()
        };

        let blas = unsafe { as_device.create_acceleration_structure(&create_info, None) }.unwrap();

        let (scratch_buffer, scratch_memory) = vk.create_buffer(
            size_info.build_scratch_size,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        let info = vk::BufferDeviceAddressInfo {
            buffer: scratch_buffer,
            ..Default::default()
        };

        let scratch_address = unsafe { vk.device.get_buffer_device_address(&info) };

        let mut as_build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR {
            ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
            flags: vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE,
            mode: vk::BuildAccelerationStructureModeKHR::BUILD,
            dst_acceleration_structure: blas,
            scratch_data: vk::DeviceOrHostAddressKHR {
                device_address: scratch_address,
            },
            ..Default::default()
        };
        as_build_geometry_info = as_build_geometry_info.geometries(&geometries);

        let range_info = vk::AccelerationStructureBuildRangeInfoKHR {
            primitive_count: max_primitive_count,
            primitive_offset: 0,
            first_vertex: 0,
            transform_offset: 0,
        };

        let cmd = vk.begin_temp_graphics_cmd();

        unsafe {
            as_device.cmd_build_acceleration_structures(
                cmd,
                &[as_build_geometry_info],
                &[&[range_info]],
            )
        };

        vk.end_temp_graphics_cmd(cmd);

        let address_info = vk::AccelerationStructureDeviceAddressInfoKHR {
            acceleration_structure: blas,
            ..Default::default()
        };

        let blas_address =
            unsafe { as_device.get_acceleration_structure_device_address(&address_info) };

        unsafe {
            vk.device.destroy_buffer(scratch_buffer, None);
            vk.device.free_memory(scratch_memory, None);
        }

        VulkanBlas {
            blas,
            buffer: blas_buffer,
            memory: blas_memory,
            address: blas_address,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        match &mut self.bvh {
            BvhType::RayQuery(vulkan_blas) => {
                assert!(!vulkan_blas.blas.is_null());
                assert!(!vulkan_blas.memory.is_null());
                assert!(!vulkan_blas.buffer.is_null());

                let Some(as_device) = &vk.as_device else {
                    unreachable!("expected as device");
                };

                unsafe {
                    vk.device.destroy_buffer(vulkan_blas.buffer, None);
                    vk.device.free_memory(vulkan_blas.memory, None);
                    as_device.destroy_acceleration_structure(vulkan_blas.blas, None);
                };

                vulkan_blas.address = 0;
            }
            BvhType::CwBvh => todo!(),
        }

        assert!(!self.vertex_buffer.is_null());
        assert!(!self.vertex_memory.is_null());

        assert!(!self.index_buffer.is_null());
        assert!(!self.index_memory.is_null());

        unsafe {
            vk.device.destroy_buffer(self.vertex_buffer, None);
            vk.device.free_memory(self.vertex_memory, None);

            vk.device.destroy_buffer(self.index_buffer, None);
            vk.device.free_memory(self.index_memory, None);
        };

        self.vertex_buffer = vk::Buffer::null();
        self.vertex_memory = vk::DeviceMemory::null();

        self.index_buffer = vk::Buffer::null();
        self.index_memory = vk::DeviceMemory::null();

        self.index_address = 0;
        self.vertex_address = 0;
    }
}
