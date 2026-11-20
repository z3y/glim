#include "includes/core.slang"

// Lightmaps store irradiance and Unity multiplies the albedo in at runtime, per RGB channel.
// This rescales each texel's irradiance so that the runtime RGB multiply gives the spectral
// product of the irradiance and the texel's albedo (like the spectral preview). The bake passes
// accumulate that product next to the lightmap, see SpectralAlbedoCorrection.
// Runs on the readback copy, the bake buffer keeps the physical irradiance for light probe bounces.

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint texelCount;
    uint combinedSH;
    // 1: the lightmap is shaded with albedo + SpectralAlbedoOffset(albedo), see GlimSpectral.hlsl
    uint albedoOffset;
    uint pad0;

    float *lightmap;
    // irradiance times the texel's albedo, multiplied spectrally
    float *reflected;
};

float3 LoadTexel(float *buffer, uint index)
{
    return float3(buffer[index * 3 + 0], buffer[index * 3 + 1], buffer[index * 3 + 2]);
}

void StoreTexel(uint index, float3 value)
{
    Push.lightmap[index * 3 + 0] = value.x;
    Push.lightmap[index * 3 + 1] = value.y;
    Push.lightmap[index * 3 + 2] = value.z;
}

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.texelCount || !SpectralEnabled())
    {
        return;
    }

    // per sample out of gamut values can leave a slightly negative average
    float3 irradiance = max(LoadTexel(Push.lightmap, index), 0.0);

    float3 albedo = 0.0;
    float3 factor = 1.0;
    // moved texels only store a position, their surface is unknown
    if (TexelAlbedo(index, albedo))
    {
        factor = SpectralAlbedoCorrection(irradiance, LoadTexel(Push.reflected, index), albedo, Push.albedoOffset != 0);
    }

    StoreTexel(index, irradiance * factor);

    if (Push.combinedSH != 0)
    {
        // L0 of the SH lightmap, L1 is stored as luminance and stays as is
        uint indexL0 = index + Push.texelCount;
        StoreTexel(indexL0, max(LoadTexel(Push.lightmap, indexL0), 0.0) * factor);
    }
}
