#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint texelCount;
    uint sampleIndex;
    uint sampleCount;
    uint mixed;

    float *lightmap;
    // spectral rendering: irradiance times the texel's albedo, see SpectralAlbedoCorrection
    float *reflected;

    Light *lights;
    uint lightsStart;
    uint lightsEnd;
};

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.texelCount)
    {
        return;
    }

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.sampleCount;
    uint lightsStart = Push.lightsStart;
    uint lightsEnd = Push.lightsEnd;
    bool mixed = Push.mixed == 1;
    float sampleScale = 1.0 / float(sampleCount);

    float3 positionWS;
    float3 normalWS;
    uint3 indexing;
    ShadowTerminator terminator;
    CreateSample(index, sampleIndex, sampleCount, positionWS, normalWS, indexing, terminator);

    float2 xiLight = SobolBurleySample2D(indexing.x, DIMENSION_LIGHT, indexing.y, indexing.z);

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;
    SHProbeL2 sh = SHProbeL2::zero();

    // mixed lights only feed the RGB bounce buffer, their direct light is realtime
    bool spectral = SpectralEnabled() && !mixed;
    float3 reflected = 0.0;
    float3 albedo = 0.0;
    bool hasAlbedo = spectral && TexelAlbedo(index, albedo);

    Light *lights = Push.lights;
    for (uint lightIndex = lightsStart; lightIndex < lightsEnd; lightIndex++)
    {
        Light light = lights[lightIndex];
        float3 L;
        float2 xi = DecorrelateSample2D(xiLight, lightIndex);
        float3 current = ShadeLightDirect(light, positionWS, normalWS, xi, throughput, L, false, terminator);
        float NoL = saturate(dot(L, normalWS));
        color.rgb += current * NoL;
        dominantDirection.xyz += L * Luminance(current * NoL * sampleScale);
        AccumulateL1LightmapDirect(sh, current, L, normalWS);

        if (spectral)
        {
            // same wavelengths as the shadow ray in ShadeLightDirect
            float u = SpectralUFromSample(xi, light.data0);
            reflected += ReflectedAtTexel(current * NoL, hasAlbedo, albedo, u, light.spectrum);
        }
    }

    ValidateSampleColor(color);

    if (spectral && all(isfinite(reflected)))
    {
        Push.reflected[index * 3 + 0] += reflected.x * sampleScale;
        Push.reflected[index * 3 + 1] += reflected.y * sampleScale;
        Push.reflected[index * 3 + 2] += reflected.z * sampleScale;
    }

    float *lightmap = Push.lightmap;

    lightmap[index * 3 + 0] += color.x * sampleScale;
    lightmap[index * 3 + 1] += color.y * sampleScale;
    lightmap[index * 3 + 2] += color.z * sampleScale;

    if (!mixed)
    {
        if (LIGHTMAP_MODE == LightmapMode::CombinedSH)
        {
            float3 L0 = sh.sh[0].xyz * sampleScale;

            // write L0
            uint index = index + Push.texelCount;
            lightmap[index * 3 + 0] += L0.x;
            lightmap[index * 3 + 1] += L0.y;
            lightmap[index * 3 + 2] += L0.z;

            float3 combinedSH = EncodeCombinedSHL1(sh);
            combinedSH *= sampleScale;

            // write L1
            index += Push.texelCount;
            lightmap[index * 3 + 0] += combinedSH.x;
            lightmap[index * 3 + 1] += combinedSH.y;
            lightmap[index * 3 + 2] += combinedSH.z;
        }
        else if (LIGHTMAP_MODE == LightmapMode::DominantDirection)
        {
            uint index = index + Push.texelCount;
            lightmap[index * 3 + 0] += dominantDirection.x;
            lightmap[index * 3 + 1] += dominantDirection.y;
            lightmap[index * 3 + 2] += dominantDirection.z;
        }
    }
}
