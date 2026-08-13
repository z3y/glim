#pragma once

#include "light.slang"
#include "mesh.slang"
#include "sh.slang"

#ifndef SOFTWARE_RT
[[vk::binding(0, 0)]]
RaytracingAccelerationStructure TopLevelAS;
#else
[[vk::binding(0, 0)]]
StructuredBuffer<float4> CWBVHNodes;
[[vk::binding(1, 0)]]
StructuredBuffer<float4> CWBVHTriangles;
#endif

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

[[vk::binding(7, 0)]]
RWStructuredBuffer<SHProbeL2> SHProbes;

[[vk::binding(10, 0)]]
StructuredBuffer<Light> Lights;

[[vk::binding(14, 0)]]
RWStructuredBuffer<float> DominantDirection;

[[vk::binding(15, 0)]]
RWStructuredBuffer<uint2> CompactionBuffer;

[[vk::binding(16, 0)]]
RWStructuredBuffer<float4> CompactedVisibility;

[[vk::binding(17, 0)]]
RWStructuredBuffer<float4> DecompactTarget;

[[vk::binding(20, 0)]]
[[vk::image_format("rgba32f")]]
TextureCube<float4> Skybox;

[[vk::binding(21, 0)]]
SamplerState SkyboxSampler;

[vk::constant_id(0)]
const uint USE_CAMERA;
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

[vk::constant_id(11)]
const uint64_t VERTEX_ADDRESS;
static let Vertices = Ptr<Vertex, Access.Read>(VERTEX_ADDRESS);

[vk::constant_id(12)]
const uint64_t INDICES_ADDRESS;
static let Indices = Ptr<uint, Access.Read>(INDICES_ADDRESS);

[vk::constant_id(13)]
const uint64_t EMISSIVE_TRIANGLES_ADDRESS;
static let EmissiveTriangles = Ptr<uint, Access.Read>(EMISSIVE_TRIANGLES_ADDRESS);

[vk::constant_id(14)]
const uint64_t COMPACTED_LIGHTMAP_ADDRESS;
static let CompactedLightmap = Ptr<float, Access.ReadWrite>(COMPACTED_LIGHTMAP_ADDRESS);

[vk::constant_id(15)]
const uint64_t LIGHTMAPS_INFO_ADDRESS;
struct LightmapInfo
{
    uint2 resolution;
    uint compactionOffset;
    uint pad;
};
static let LightmapsInfo = Ptr<LightmapInfo, Access.Read>(LIGHTMAPS_INFO_ADDRESS);

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
