#include "includes/core.slang"

// Lightmaps store irradiance and Unity multiplies the albedo in at runtime, per RGB channel.
// This rescales each texel's irradiance so that the runtime RGB multiply gives the spectral
// product of the irradiance and the texel's albedo (like the spectral preview), see
// SpectralAlbedoCorrection. Runs on the readback copy, the bake buffer keeps the physical
// irradiance for light probe bounces.

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint texelCount;
    uint combinedSH;
    uint pad0;
    uint pad1;

    float *lightmap;
};

float3 LoadTexel(uint index)
{
    return float3(Push.lightmap[index * 3 + 0], Push.lightmap[index * 3 + 1], Push.lightmap[index * 3 + 2]);
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

    float4 visibility = Push.rt.visibility[index];
    bool moved = asuint(visibility.w) != 0u;
    if (moved)
    {
        // only a position is stored for moved texels, their surface is unknown
        return;
    }

    float2 bary = visibility.xy;
    uint primitiveID = asuint(visibility.z);
    bool frontFace = index < BACKFACE_TEXEL_START;

    Surface surface = Surface::Get(primitiveID, bary, frontFace);
    float3 albedo = SampleAlbedo(surface.coords, surface.lightmapGroup).rgb;

    float3 irradiance = LoadTexel(index);
    float3 factor = SpectralAlbedoCorrection(irradiance, albedo);

    if (any(factor != 1.0))
    {
        StoreTexel(index, irradiance * factor);

        if (Push.combinedSH != 0)
        {
            // L0 of the SH lightmap, L1 is stored as luminance and stays as is
            uint indexL0 = index + Push.texelCount;
            StoreTexel(indexL0, LoadTexel(indexL0) * factor);
        }
    }
}
