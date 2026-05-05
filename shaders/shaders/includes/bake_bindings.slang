#include "mesh.slang"
#include "lights.slang"

#ifndef SOFTWARE_RT
[[vk::binding(0, 0)]]
RaytracingAccelerationStructure TopLevelAS;
#else
[[vk::binding(0, 0)]]
StructuredBuffer<float4> CWBVHNodes;
[[vk::binding(1, 0)]]
StructuredBuffer<float4> CWBVHTriangles;
#endif

[[vk::image_format("rgba32f")]]
[[vk::binding(2, 0)]]
RWTexture2D<float4> VisibilityBuffer;

[[vk::binding(3, 0)]]
Texture2D<float4> Albedo[];

[[vk::binding(5, 0)]]
Texture2D<float4> Emission[];

[[vk::image_format("rgba32f")]]
[[vk::binding(4, 0)]]
RWTexture2D<float4> LightmapDiffuse;

[[vk::binding(6, 0)]]
SamplerState SamplerLinearClamp;


// maybe NonUniformResourceIndex()?
float4 SampleAlbedo(float2 uv, uint lightmapGroup)
{
    return Albedo[lightmapGroup].SampleLevel(SamplerLinearClamp, uv, 0);
}

float4 SampleEmission(float2 uv, uint lightmapGroup)
{
    return Emission[lightmapGroup].SampleLevel(SamplerLinearClamp, uv, 0);
}
