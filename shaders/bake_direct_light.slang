#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint compactedCount;
    uint sampleIndex;
    uint maxSamples;
    uint pad0;

    uint lightsStart;
    uint lightsCount;
    uint pad1;
    uint pad2;
};

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.compactedCount)
    {
        return;
    }

    float4 visibility = CompactedVisibility[index];

    float3 positionWS = visibility.xyz;
    float3 normalWS = UnpackNormalOctahedron(visibility.w);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint lightsStart = Push.lightsStart;
    uint lightsCount = Push.lightsCount;
    float sampleScale = 1.0 / float(sampleCount);

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit1D(index);
    uint sobolIndexMask = ~0u;
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    float2 xiLight = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;
    SHProbeL2 sh = SHProbeL2::zero();

    for (uint lightIndex = lightsStart; lightIndex < lightsCount; lightIndex++)
    {
        Light light = Lights[lightIndex];
        float3 L;
        float3 current = ShadeLightDirect(light, positionWS, normalWS, xiLight, throughput, L, false);
        float3 NoL = saturate(dot(L, normalWS));
        color.rgb += current * NoL;
        dominantDirection.xyz += L * Luminance(current * NoL);

        AccumulateL1LightmapDirect(sh, current, L, normalWS);
    }

    ValidateSampleColor(color);

    CompactedLightmap[index * 3 + 0] += color.x * sampleScale;
    CompactedLightmap[index * 3 + 1] += color.y * sampleScale;
    CompactedLightmap[index * 3 + 2] += color.z * sampleScale;

    if (LIGHTMAP_MODE == LightmapMode::CombinedSH)
    {
        float3 L0 = sh.sh[0].xyz * sampleScale;

        // write L0
        uint index = index + Push.compactedCount;
        CompactedLightmap[index * 3 + 0] += L0.x;
        CompactedLightmap[index * 3 + 1] += L0.y;
        CompactedLightmap[index * 3 + 2] += L0.z;

        float3 combinedSH = EncodeCombinedSHL1(sh);
        combinedSH *= sampleScale;

        // write L1
        index += Push.compactedCount;
        CompactedLightmap[index * 3 + 0] += combinedSH.x;
        CompactedLightmap[index * 3 + 1] += combinedSH.y;
        CompactedLightmap[index * 3 + 2] += combinedSH.z;
    }
    else if (LIGHTMAP_MODE == LightmapMode::DominantDirection)
    {
        uint index = index + Push.compactedCount;
        CompactedLightmap[index * 3 + 0] += dominantDirection.x;
        CompactedLightmap[index * 3 + 1] += dominantDirection.y;
        CompactedLightmap[index * 3 + 2] += dominantDirection.z;
    }
}
