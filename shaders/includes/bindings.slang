#pragma once

#include "constants.slang"
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

[[vk::binding(8, 0)]]
Texture2D<float> Alphas[];

[[vk::binding(20, 0)]]
[[vk::image_format("rgba32f")]]
TextureCube<float4> Skybox;

[[vk::binding(21, 0)]]
SamplerState SkyboxSampler;

[[vk::binding(6, 0)]]
Texture2D<float4> Cookies[];

[[vk::binding(7, 0)]]
SamplerState LinearSampler;

struct RayTracingData
{
    uint *indices;
    Vertex *vertices;

    float4 *BVHNodes;
    float4 *BVHTriangles;

    float4 *visibility;
    float4 *mainUVs;
};

float4 SampleAlbedo(uint2 coords, uint lightmapGroup)
{
    uint index = LIGHTMAP_GROUP_COUNT <= 1 ? 0 : NonUniformResourceIndex(lightmapGroup);
    Texture2D tex = Albedo[index];
    return saturate(tex[coords]);
}

float SampleAlpha(float2 uv, uint alphaIndex, bool wrapUV)
{
    uint index = NonUniformResourceIndex(alphaIndex);
    Texture2D<float> tex = Alphas[index];

    uint width, height;
    tex.GetDimensions(width, height);
    float2 res = float2(width, height);

    uv = wrapUV ? frac(uv) : saturate(uv);

    uint2 texel = min((uint2)(uv * res), uint2(width - 1, height - 1));

    return saturate(tex[texel].r);
}

float4 SampleEmission(uint2 coords, uint lightmapGroup)
{
    uint index = LIGHTMAP_GROUP_COUNT <= 1 ? 0 : NonUniformResourceIndex(lightmapGroup);
    Texture2D tex = Emission[index];
    return max(0.0, tex[coords] * EMISSIVE_MULTIPLIER);
}

float4 SampleCookie(float2 uv, uint index)
{
    Texture2D tex = Cookies[NonUniformResourceIndex(index)];
    return tex.SampleLevel(LinearSampler, saturate(uv), 0);
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
