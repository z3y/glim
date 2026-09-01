#pragma once

#include "light.slang"
#include "mesh.slang"
#include "sh.slang"

[[vk::binding(0, 0)]]
RaytracingAccelerationStructure TopLevelAS;

[[vk::binding(2, 0)]]
[[vk::image_format("rgba32f")]]
RWTexture2D<float4> VisibilityBuffer;

[[vk::binding(3, 0)]]
Texture2D<float4> Albedo[];

[[vk::binding(5, 0)]]
Texture2D<float4> Emission[];

[[vk::binding(4, 0)]]
[[vk::image_format("rgba32f")]]
RWTexture2D<float4> LightmapDiffuse;

[[vk::binding(20, 0)]]
[[vk::image_format("rgba32f")]]
TextureCube<float4> Skybox;

[[vk::binding(21, 0)]]
SamplerState SkyboxSampler;

struct LightmapInfo
{
    uint2 resolution;
    uint compactionOffset;
    uint pad;
};

#define DECLARE_CONST_PTR(name, type, lo_id) \
    [vk::constant_id(lo_id)]                 \
    const uint name##_ADDRESS_LO;            \
    [vk::constant_id((lo_id) + 1)]           \
    const uint name##_ADDRESS_HI;            \
    static let name = reinterpret<Ptr<type, Access.ReadWrite>>(uint2(name##_ADDRESS_LO, name##_ADDRESS_HI));

[vk::constant_id(0)]
const uint HARDWARE_RT;

[vk::constant_id(1)]
const uint LIGHT_FALLOFF_TYPE;

[vk::constant_id(2)]
const uint TRANSPARENT_PRIMITIVE_ID_OFFSET;

[vk::constant_id(3)]
const uint EMISSIVE_TRIANGLES_COUNT;

[vk::constant_id(4)]
const uint MULTIPLE_IMPORTANCE_SAMPLING;

[vk::constant_id(5)]
const uint LIGHTMAP_GROUP_COUNT;

[vk::constant_id(6)]
const uint LIGHTMAP_MODE;

[vk::constant_id(7)]
const uint COORDINATE_SPACE;

[vk::constant_id(8)]
const float SKYBOX_INTENSITY;

[vk::constant_id(9)]
const float INDIRECT_INTENSITY;

[vk::constant_id(10)]
const float LIGHTPROBE_DERINGING;

DECLARE_CONST_PTR(Vertices, Vertex, 12)
DECLARE_CONST_PTR(Indices, uint, 14)
DECLARE_CONST_PTR(EmissiveTriangles, uint, 16)
DECLARE_CONST_PTR(CompactedLightmap, float, 18)
DECLARE_CONST_PTR(LightmapsInfo, LightmapInfo, 20)
DECLARE_CONST_PTR(CompactedVisibility, float4, 22)
DECLARE_CONST_PTR(Lights, Light, 24)
DECLARE_CONST_PTR(CompactionBuffer, uint2, 26)
DECLARE_CONST_PTR(BVHNodes, float4, 28)
DECLARE_CONST_PTR(BVHTriangles, float4, 30)

enum LightmapMode : uint
{
    NonDirectional = 0,
    DominantDirection = 1,
    CombinedSH = 3,
};

float4 SampleAlbedo(float2 uv, uint lightmapGroup)
{
    uint index = LIGHTMAP_GROUP_COUNT <= 1 ? 0 : NonUniformResourceIndex(lightmapGroup);
    Texture2D tex = Albedo[index];
    return saturate(tex[(uint2)uv]);
}

float4 SampleEmission(float2 uv, uint lightmapGroup)
{
    uint index = LIGHTMAP_GROUP_COUNT <= 1 ? 0 : NonUniformResourceIndex(lightmapGroup);
    Texture2D tex = Emission[index];
    return max(0.0, tex[(uint2)uv]);
}

float3 SampleSkybox(float3 dir)
{
    if (COORDINATE_SPACE == 1)
    {
        dir.z = -dir.z;
    }
    const float SKYBOX_MAX = 10.0;

    return clamp(Skybox.SampleLevel(SkyboxSampler, dir, 0.0).rgb * SKYBOX_INTENSITY, 0.0, SKYBOX_MAX);
}

uint2 GetBounceBufferOffsets(uint compactedCount)
{
    uint2 offsets = uint2(compactedCount, compactedCount * 2);

    if (LIGHTMAP_MODE == LightmapMode::DominantDirection)
    {
        offsets += compactedCount;
    }
    else if (LIGHTMAP_MODE == LightmapMode::CombinedSH)
    {
        offsets += compactedCount * 2;
    }

    return offsets;
}
