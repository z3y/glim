#pragma once

#include "lights.slang"
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

[[vk::binding(6, 0)]]
SamplerState TextureSampler;

[[vk::binding(7, 0)]]
RWStructuredBuffer<SHProbeL2> SHProbes;

[[vk::binding(8, 0)]]
StructuredBuffer<uint> Indices;

[[vk::binding(9, 0)]]
StructuredBuffer<Vertex> Vertices;

[[vk::binding(10, 0)]]
StructuredBuffer<Light> Lights;

[[vk::binding(11, 0)]]
[[vk::image_format("rgba32f")]]
RWTexture2D<float4> NormalsBuffer;

[[vk::binding(12, 0)]]
StructuredBuffer<uint> EmissiveTriangles;

[[vk::binding(13, 0)]]
[[vk::image_format("rgba32f")]]
Texture2D<float4> PreviousDiffuse[];

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

float4 SampleAlbedo(float2 uv, uint lightmapGroup)
{
    return Albedo[NonUniformResourceIndex(lightmapGroup)].SampleLevel(TextureSampler, uv, 0);
}

float4 SampleEmission(float2 uv, uint lightmapGroup)
{
    return Emission[NonUniformResourceIndex(lightmapGroup)].SampleLevel(TextureSampler, uv, 0);
}

float4 SamplePreviousDiffuse(float2 uv, uint lightmapGroup)
{
    return PreviousDiffuse[NonUniformResourceIndex(lightmapGroup)].SampleLevel(TextureSampler, uv, 0);
}
