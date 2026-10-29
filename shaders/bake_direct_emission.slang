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

    float *texelValidity;
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

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    float sampleScale = 1.0 / float(sampleCount);

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit1D(index);
    uint sobolIndexMask = ~0u;
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    float2 xiAA = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);
    float3 positionWS;
    float3 normalWS;
    CreateSample(index, xiAA, positionWS, normalWS);

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;
    SHProbeL2 sh = SHProbeL2::zero();

    float2 xiBounce = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z);

    Ray ray;
    ray.P = positionWS;
    ray.D = RandomDirectionInHemisphereCosine(normalWS, xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    float randomEmissiveTrianglePdf = 1.0 / float(EMISSIVE_TRIANGLES_COUNT);
    float bsdfPdf = 1.0 / float.getPi();

    if (EMISSIVE_TRIANGLES_COUNT > 0)
    {
        float2 xi = SobolBurleySample2D(indexing.x, 2, indexing.y, indexing.z);
        uint triIndex = min(uint(xi.x * float(EMISSIVE_TRIANGLES_COUNT)), EMISSIVE_TRIANGLES_COUNT - 1);
        uint emissivePrimitiveID = EmissiveTriangles[triIndex];

        float3 L;
        float pdf = 0.0;
        float3 current = ShadeTriangleLight(emissivePrimitiveID, positionWS, normalWS, xi.y, throughput, randomEmissiveTrianglePdf, pdf, L, false);
        float3 NoL = saturate(dot(L, normalWS));

        current *= MISWeight(pdf, bsdfPdf);
        color.rgb += current * NoL;
        dominantDirection.xyz += L * Luminance(current * NoL);
        AccumulateL1LightmapDirect(sh, current, L, normalWS);
    }

    float validity = sampleScale;

    Hit hit;
    if (TraceRay(ray, hit))
    {
        Surface surface = Surface::Get(hit.primitiveID, hit.bary, hit.frontFace);

        if (!hit.frontFace)
        {
            if (!surface.backfaceGI)
            {
                color.a = 0.0;
                validity = 0.0;
            }
        }

        if (surface.emissive)
        {
            float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
            float3 emission = bounceEmission * color.a;

            if (any(emission > 0.0))
            {
                if (EMISSIVE_TRIANGLES_COUNT > 0)
                {
                    float triangleArea = CalculateTriangleArea(hit.primitiveID);
                    float3 positionToLight = surface.P - positionWS;
                    float distSq = max(dot(positionToLight, positionToLight), 0.0001);
                    float cosThetaLight = saturate(dot(-normalize(positionToLight), surface.N));

                    if (cosThetaLight > 0.0)
                    {
                        float lightPdf = (randomEmissiveTrianglePdf / triangleArea) * distSq / cosThetaLight;
                        emission *= MISWeight(bsdfPdf, lightPdf);
                        color.rgb += emission;
                        dominantDirection.xyz += ray.D * Luminance(emission);
                        AccumulateL1LightmapIndirect(sh, emission, ray.D, normalWS);
                    }
                }
                else
                {
                    color.rgb += emission;
                    dominantDirection.xyz += ray.D * Luminance(emission);
                    AccumulateL1LightmapIndirect(sh, emission, ray.D, normalWS);
                }
            }
        }
    }
    else
    {
        float3 current = SampleSkybox(ray.D);
        color.rgb += current;
        dominantDirection.xyz += ray.D * Luminance(current);
        AccumulateL1LightmapIndirect(sh, current, ray.D, normalWS);
    }

    Push.texelValidity[index] += validity;

    // ValidateSampleColor(color);

    CompactedLightmap[index * 3 + 0] += color.x * sampleScale;
    CompactedLightmap[index * 3 + 1] += color.y * sampleScale;
    CompactedLightmap[index * 3 + 2] += color.z * sampleScale;

    // todo
    // if (LIGHTMAP_MODE == LightmapMode::CombinedSH)
    // {
    //     float3 L0 = sh.sh[0].xyz * sampleScale;

    //     // write L0
    //     uint index = index + Push.compactedCount;
    //     CompactedLightmap[index * 3 + 0] += L0.x;
    //     CompactedLightmap[index * 3 + 1] += L0.y;
    //     CompactedLightmap[index * 3 + 2] += L0.z;

    //     float3 combinedSH = EncodeCombinedSHL1(sh);
    //     combinedSH *= sampleScale;

    //     // write L1
    //     index += Push.compactedCount;
    //     CompactedLightmap[index * 3 + 0] += combinedSH.x;
    //     CompactedLightmap[index * 3 + 1] += combinedSH.y;
    //     CompactedLightmap[index * 3 + 2] += combinedSH.z;
    // }
    // else if (LIGHTMAP_MODE == LightmapMode::DominantDirection)
    // {
    //     uint index = index + Push.compactedCount;
    //     CompactedLightmap[index * 3 + 0] += dominantDirection.x;
    //     CompactedLightmap[index * 3 + 1] += dominantDirection.y;
    //     CompactedLightmap[index * 3 + 2] += dominantDirection.z;
    // }

    bool lastSample = sampleIndex == (sampleCount - 1);
    [branch]
    if (lastSample)
    {
        const float validityThreshold = 0.8;
        if (Push.texelValidity[index] < validityThreshold)
        {
            // set primitive id to max value
            float4 visibility = CompactedVisibility[index];
            visibility.z = asfloat(uint::maxValue);
            // CompactedVisibility[index] = visibility;

            // CompactedLightmap[index * 3 + 0] = asfloat(uint::maxValue);
        }
    }
}
