use ash::vk::{self, Handle};

use crate::{
    CoordinateSystem,
    buffer::Buffer,
    math::*,
    seams::{Seam, find_seams},
    vulkan_context::VulkanContext,
};
use core::slice;

#[repr(C)]
pub struct FfiMesh {
    pub vertices: *const Vector3,
    pub normals: *const Vector3,
    pub uvs: *const Vector2,
    pub indices: *const u32,
    pub vertices_length: u32,
    pub indices_length: u32,
    pub lightmap_group: u32,
    pub backface_gi: bool,
    pub transparent: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: Vector3,
    pub flags: u32,
    pub normal: Vector2,
    pub uv: Vector2,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

fn sign_not_zero(v: f32) -> f32 {
    if v >= 0.0 { 1.0 } else { -1.0 }
}

pub fn encode_normal_octahedron(n: Vector3) -> Vector2 {
    let inv_l1 = 1.0 / (n.x.abs() + n.y.abs() + n.z.abs());

    let mut p = Vector2::new(n.x * inv_l1, n.y * inv_l1);

    if n.z < 0.0 {
        let x = (1.0 - p.y.abs()) * sign_not_zero(p.x);
        let y = (1.0 - p.x.abs()) * sign_not_zero(p.y);

        p = Vector2::new(x, y);
    }

    p
}

impl Mesh {
    pub fn append_ffi_mesh(
        &mut self,
        mesh: FfiMesh,
        system: CoordinateSystem,
        all_seams: &mut Vec<Seam>,
        add_seams: bool,
    ) {
        let positions =
            unsafe { slice::from_raw_parts(mesh.vertices, mesh.vertices_length as usize) };
        let normals = unsafe { slice::from_raw_parts(mesh.normals, mesh.vertices_length as usize) };
        let uvs = unsafe { slice::from_raw_parts(mesh.uvs, mesh.vertices_length as usize) };
        let indices = unsafe { slice::from_raw_parts(mesh.indices, mesh.indices_length as usize) };

        // let sample_scale = 20.0;
        let unity = system == CoordinateSystem::Unity;

        if add_seams {
            let mut flip = true;
            if unity {
                flip = false;
            }
            let seams = find_seams(indices, positions, normals, uvs, flip, mesh.lightmap_group);
            all_seams.extend(seams);
        }

        let offset = self.vertices.len() as u32;

        let lightmap_group = mesh.lightmap_group;
        let backface_gi = mesh.backface_gi;

        self.vertices.reserve(positions.len());

        for i in 0..positions.len() {
            let mut normal = normals[i];
            normal.transform_space(system);

            let mut flags: u32 = 0;
            flags |= lightmap_group & 0xFFFF;
            if backface_gi {
                flags |= 1 << 16;
            }

            let mut uv = uvs[i];
            if unity {
                uv.y = 1.0 - uv.y;
            }

            let mut vertex = Vertex {
                position: positions[i],
                normal: encode_normal_octahedron(normal),
                uv: uv,
                flags,
            };

            vertex.position.transform_space(system);

            self.vertices.push(vertex);
        }

        match system {
            CoordinateSystem::Default => {
                self.indices.extend(indices.iter().map(|i| i + offset));
            }
            CoordinateSystem::Unity => {
                self.indices.reserve(indices.len());

                for triangle in indices.chunks(3) {
                    self.indices.push(triangle[0] + offset);
                    self.indices.push(triangle[2] + offset);
                    self.indices.push(triangle[1] + offset);
                }
            }
        }
    }

    pub fn merge_mesh(&mut self, mesh: &Mesh) {
        let offset = self.vertices.len() as u32;

        self.vertices.extend(&mesh.vertices);
        self.indices.extend(mesh.indices.iter().map(|i| i + offset));
    }
}

pub struct VulkanAs {
    acceleration_structure: vk::AccelerationStructureKHR,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    address: vk::DeviceAddress,
}

impl VulkanAs {
    pub fn acceleration_structure(&self) -> vk::AccelerationStructureKHR {
        self.acceleration_structure
    }

    pub fn null() -> Self {
        Self {
            acceleration_structure: vk::AccelerationStructureKHR::null(),
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            address: 0,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        assert!(!self.acceleration_structure.is_null());
        assert!(!self.memory.is_null());
        assert!(!self.buffer.is_null());

        let Some(as_device) = &vk.as_device else {
            unreachable!("expected as device");
        };

        unsafe {
            vk.device.destroy_buffer(self.buffer, None);
            vk.device.free_memory(self.memory, None);
            as_device.destroy_acceleration_structure(self.acceleration_structure, None);
        };

        self.address = 0;
        self.memory = vk::DeviceMemory::null();
        self.buffer = vk::Buffer::null();
        self.acceleration_structure = vk::AccelerationStructureKHR::null();
    }
}

pub enum AccelerationStructureType {
    RayQuery(VulkanAs),
    CwBvh,
}

pub struct GpuMesh {
    pub vertex_buffer: Buffer,
    pub index_buffer: Buffer,

    pub acceleration_structure: AccelerationStructureType,

    pub index_len: u32,
}

impl GpuMesh {
    pub fn new(vk: &VulkanContext, opaque_mesh: &Mesh, transparent_mesh: &Mesh) -> Self {
        let mut usage = vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;

        if vk.as_device.is_some() {
            usage |= vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR;
        }

        let opaque_triangle_count = (opaque_mesh.indices.len() / 3) as u32;
        let transparent_triangle_count = (transparent_mesh.indices.len() / 3) as u32;

        let mut merged_mesh = opaque_mesh.clone();
        merged_mesh.merge_mesh(transparent_mesh);

        // vertices
        let vertex_buffer = Buffer::new(
            vk,
            String::from("Vertex"),
            &merged_mesh.vertices,
            usage,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
        let index_buffer = Buffer::new(
            vk,
            String::from("Index"),
            &merged_mesh.indices,
            usage,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        let bvh = if vk.as_device.is_some() {
            AccelerationStructureType::RayQuery(GpuMesh::create_vulkan_blas(
                vk,
                &merged_mesh,
                vertex_buffer.address,
                index_buffer.address,
                opaque_triangle_count,
                transparent_triangle_count,
            ))
        } else {
            AccelerationStructureType::CwBvh // todo
        };

        Self {
            vertex_buffer,
            index_buffer,
            acceleration_structure: bvh,
            index_len: merged_mesh.indices.len() as u32,
        }
    }

    pub fn create_vulkan_blas(
        vk: &VulkanContext,
        mesh: &Mesh,
        vertex_address: vk::DeviceAddress,
        index_address: vk::DeviceAddress,
        opaque_triangle_count: u32,
        transparent_triangle_count: u32,
    ) -> VulkanAs {
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

        let mut geometries = Vec::new();
        let mut max_primitive_counts = Vec::new();
        let mut ranges = Vec::new();

        if opaque_triangle_count > 0 {
            let opaque_geometry = vk::AccelerationStructureGeometryKHR {
                geometry_type: vk::GeometryTypeKHR::TRIANGLES,
                geometry: vk::AccelerationStructureGeometryDataKHR {
                    triangles: triangles,
                },
                flags: vk::GeometryFlagsKHR::OPAQUE,
                ..Default::default()
            };
            geometries.push(opaque_geometry);
            max_primitive_counts.push(opaque_triangle_count);
        }

        if transparent_triangle_count > 0 {
            let transparent_geometry = vk::AccelerationStructureGeometryKHR {
                geometry_type: vk::GeometryTypeKHR::TRIANGLES,
                geometry: vk::AccelerationStructureGeometryDataKHR {
                    triangles: triangles,
                },
                flags: vk::GeometryFlagsKHR::NO_DUPLICATE_ANY_HIT_INVOCATION,
                ..Default::default()
            };
            geometries.push(transparent_geometry);
            max_primitive_counts.push(transparent_triangle_count);
        }

        let build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(&geometries);

        let mut size_info = vk::AccelerationStructureBuildSizesInfoKHR::default();

        let Some(as_device) = &vk.as_device else {
            unreachable!("expected as device");
        };

        unsafe {
            as_device.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &build_info,
                &max_primitive_counts,
                &mut size_info,
            )
        };

        let (blas_buffer, blas_memory, _) = vk.create_buffer(
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

        let (scratch_buffer, scratch_memory, _) = vk.create_buffer(
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

        let opaque_range = vk::AccelerationStructureBuildRangeInfoKHR {
            primitive_count: opaque_triangle_count,
            primitive_offset: 0,
            first_vertex: 0,
            transform_offset: 0,
        };

        let transparent_range = vk::AccelerationStructureBuildRangeInfoKHR {
            primitive_count: transparent_triangle_count,
            primitive_offset: opaque_triangle_count * 3 * size_of::<u32>() as u32,
            first_vertex: 0,
            transform_offset: 0,
        };

        if opaque_triangle_count > 0 {
            ranges.push(opaque_range)
        }

        if transparent_triangle_count > 0 {
            ranges.push(transparent_range)
        }

        let cmd = vk.begin_single_use_cmd();

        unsafe {
            as_device.cmd_build_acceleration_structures(cmd, &[as_build_geometry_info], &[&ranges])
        };

        vk.end_single_use_cmd(cmd);

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

        VulkanAs {
            acceleration_structure: blas,
            buffer: blas_buffer,
            memory: blas_memory,
            address: blas_address,
        }
    }

    pub fn null() -> Self {
        Self {
            acceleration_structure: AccelerationStructureType::RayQuery(VulkanAs::null()),
            index_len: 0,
            vertex_buffer: Buffer::null(),
            index_buffer: Buffer::null(),
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        match &mut self.acceleration_structure {
            AccelerationStructureType::RayQuery(vulkan_blas) => {
                vulkan_blas.destroy(vk);
            }
            AccelerationStructureType::CwBvh => todo!(),
        }

        self.index_buffer.destroy(vk);
        self.vertex_buffer.destroy(vk);
    }
}

pub fn create_tlas(vk: &VulkanContext, blas: &VulkanAs) -> VulkanAs {
    let Some(as_device) = &vk.as_device else {
        unreachable!("expected as device");
    };

    let as_instance = vk::AccelerationStructureInstanceKHR {
        transform: vk::TransformMatrixKHR {
            matrix: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        },
        instance_custom_index_and_mask: vk::Packed24_8::new(0, 0xFF),
        instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(
            0,
            vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
        ),
        acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
            device_handle: blas.address,
        },
    };

    let (as_instance_buffer, as_instance_mem, _) = vk.create_buffer(
        std::mem::size_of::<vk::AccelerationStructureInstanceKHR>() as vk::DeviceSize,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
            | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );

    let instances = [as_instance];
    let (_, bytes, _) = unsafe { instances.align_to::<u8>() };
    vk.upload_buffer(bytes, as_instance_buffer);

    let address_info = vk::BufferDeviceAddressInfo {
        buffer: as_instance_buffer,
        ..Default::default()
    };

    let as_instance_address = unsafe { vk.device.get_buffer_device_address(&address_info) };

    let top_geometry = vk::AccelerationStructureGeometryKHR {
        geometry_type: vk::GeometryTypeKHR::INSTANCES,
        geometry: vk::AccelerationStructureGeometryDataKHR {
            instances: vk::AccelerationStructureGeometryInstancesDataKHR {
                s_type: vk::StructureType::ACCELERATION_STRUCTURE_GEOMETRY_INSTANCES_DATA_KHR,
                p_next: std::ptr::null(),
                array_of_pointers: vk::FALSE,
                data: vk::DeviceOrHostAddressConstKHR {
                    device_address: as_instance_address,
                },
                ..Default::default()
            },
        },
        flags: vk::GeometryFlagsKHR::OPAQUE,
        ..Default::default()
    };

    let geometries = [top_geometry];
    let mut top_build_info = vk::AccelerationStructureBuildGeometryInfoKHR {
        ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
        flags: vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE,
        mode: vk::BuildAccelerationStructureModeKHR::BUILD,
        ..Default::default()
    };
    top_build_info = top_build_info.geometries(&geometries);

    let mut size_info = vk::AccelerationStructureBuildSizesInfoKHR {
        ..Default::default()
    };

    let instances_counts = [1];
    unsafe {
        as_device.get_acceleration_structure_build_sizes(
            vk::AccelerationStructureBuildTypeKHR::DEVICE,
            &top_build_info,
            &instances_counts,
            &mut size_info,
        )
    };

    let (tlas_buffer, tlas_mem, _) = vk.create_buffer(
        size_info.acceleration_structure_size,
        vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
            | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );

    let tlas_create_info = vk::AccelerationStructureCreateInfoKHR {
        buffer: tlas_buffer,
        size: size_info.acceleration_structure_size,
        ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
        ..Default::default()
    };

    let tlas = unsafe {
        as_device
            .create_acceleration_structure(&tlas_create_info, None)
            .unwrap()
    };

    let (scratch_buffer2, scratch_mem2, _) = vk.create_buffer(
        size_info.build_scratch_size,
        vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::STORAGE_BUFFER,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );

    let scratch_address_info2 = vk::BufferDeviceAddressInfo {
        buffer: scratch_buffer2,
        ..Default::default()
    };

    let scratch_address2 = unsafe { vk.device.get_buffer_device_address(&scratch_address_info2) };

    top_build_info.dst_acceleration_structure = tlas;
    top_build_info.scratch_data.device_address = scratch_address2;

    let range_info2 = [vk::AccelerationStructureBuildRangeInfoKHR {
        primitive_count: 1,
        ..Default::default()
    }];

    let infos = [top_build_info];
    let cmd = vk.begin_single_use_cmd();

    unsafe { as_device.cmd_build_acceleration_structures(cmd, &infos, &[&range_info2]) };

    vk.end_single_use_cmd(cmd);

    unsafe {
        vk.device.destroy_buffer(as_instance_buffer, None);
        vk.device.free_memory(as_instance_mem, None);
        vk.device.destroy_buffer(scratch_buffer2, None);
        vk.device.free_memory(scratch_mem2, None);
    }

    let as_address_info = vk::AccelerationStructureDeviceAddressInfoKHR {
        acceleration_structure: tlas,
        ..Default::default()
    };

    let tlas_address =
        unsafe { as_device.get_acceleration_structure_device_address(&as_address_info) };

    VulkanAs {
        acceleration_structure: tlas,
        buffer: tlas_buffer,
        memory: tlas_mem,
        address: tlas_address,
    }
}
