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

[[vk::image_format("rgba32f")]]
[[vk::binding(3, 0)]]
Texture2D<float4> Albedo;

[[vk::image_format("rgba32f")]]
[[vk::binding(4, 0)]]
RWTexture2D<float4> LightmapDiffuse;
