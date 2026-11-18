#include "includes/core.slang"
#include "includes/sobol_burley.slang"
#include "includes/light_tree.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint compactedCount;
    uint sampleIndex;
    uint maxSamples;
    uint pad0;

    float *lightmap;
    float4 *lightTree;
    uint *primitiveToLeaf;
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
    float invPi = 1.0 / float.getPi();

    float3 positionWS;
    float3 normalWS;
    uint3 indexing;
    CreateSample(index, sampleIndex, sampleCount, positionWS, normalWS, indexing);

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

    if (EMISSIVE_TRIANGLES_COUNT > 0)
    {
        float2 xiPick = SobolBurleySample2D(indexing.x, 2, indexing.y, indexing.z);
        float2 xiLight = SobolBurleySample2D(indexing.x, 3, indexing.y, indexing.z);

        uint emissivePrimitiveID;
        float selectionPmf;

        if (LightTreeSample(Push.lightTree, positionWS, normalWS, true, xiPick.x, emissivePrimitiveID, selectionPmf))
        {
            float3 L;
            float pdf = 0.0;
            float3 current = ShadeTriangleLight(emissivePrimitiveID, positionWS, normalWS, xiLight, throughput, selectionPmf, pdf, L, false);
            float NoL = saturate(dot(L, normalWS));

            float bsdfPdfL = NoL * invPi;

            current *= MISWeight(pdf, bsdfPdfL);
            color.rgb += current * NoL;
            dominantDirection.xyz += L * Luminance(current * NoL * sampleScale);
            AccumulateL1LightmapDirect(sh, current, L, normalWS);
        }
    }

    Hit hit;
    if (TraceRay(ray, hit))
    {
        Surface surface = Surface::Get(hit.primitiveID, hit.bary, hit.frontFace);

        if (!hit.frontFace)
        {
            if (!surface.backfaceGI)
            {
                color.a = 0.0;
            }
        }

        if (surface.emissive)
        {
            float3 emission = SampleEmission(surface.coords, surface.lightmapGroup).rgb * color.a;

            if (any(emission > 0.0))
            {
                float weight = 1.0;

                if (EMISSIVE_TRIANGLES_COUNT > 0)
                {
                    float selectionPmf = LightTreePmf(Push.lightTree, Push.primitiveToLeaf, hit.primitiveID, positionWS, normalWS, true);

                    if (selectionPmf > 0.0)
                    {
                        float3 positionToLight = surface.P - positionWS;
                        float distSq = max(dot(positionToLight, positionToLight), 0.0001);

                        // matches ShadeTriangleLight
                        float cosThetaLight = dot(-ray.D, surface.N);
                        cosThetaLight = surface.backfaceGI ? abs(cosThetaLight) : saturate(cosThetaLight);

                        if (cosThetaLight > 0.001)
                        {
                            float triangleArea = CalculateTriangleArea(hit.primitiveID);
                            float lightPdf = (selectionPmf / triangleArea) * distSq / cosThetaLight;
                            float bsdfPdfH = saturate(dot(normalWS, ray.D)) * invPi;
                            weight = MISWeight(bsdfPdfH, lightPdf);
                        }
                    }
                }

                emission *= weight;
                color.rgb += emission;
                dominantDirection.xyz += ray.D * Luminance(emission * sampleScale);
                AccumulateL1LightmapIndirect(sh, emission, ray.D, normalWS);
            }
        }
    }
    else
    {
        float3 current = SampleSkybox(ray.D);
        color.rgb += current;
        dominantDirection.xyz += ray.D * Luminance(current * sampleScale);
        AccumulateL1LightmapIndirect(sh, current, ray.D, normalWS);
    }

    ValidateSampleColor(color);

    float *lightmap = Push.lightmap;

    lightmap[index * 3 + 0] += color.x * sampleScale;
    lightmap[index * 3 + 1] += color.y * sampleScale;
    lightmap[index * 3 + 2] += color.z * sampleScale;

    if (LIGHTMAP_MODE == LightmapMode::CombinedSH)
    {
        float3 L0 = sh.sh[0].xyz * sampleScale;

        uint indexL0 = index + Push.compactedCount;
        lightmap[indexL0 * 3 + 0] += L0.x;
        lightmap[indexL0 * 3 + 1] += L0.y;
        lightmap[indexL0 * 3 + 2] += L0.z;

        float3 combinedSH = EncodeCombinedSHL1(sh);
        combinedSH *= sampleScale;

        uint indexL1 = indexL0 + Push.compactedCount;
        lightmap[indexL1 * 3 + 0] += combinedSH.x;
        lightmap[indexL1 * 3 + 1] += combinedSH.y;
        lightmap[indexL1 * 3 + 2] += combinedSH.z;
    }
    else if (LIGHTMAP_MODE == LightmapMode::DominantDirection)
    {
        uint indexDir = index + Push.compactedCount;
        lightmap[indexDir * 3 + 0] += dominantDirection.x;
        lightmap[indexDir * 3 + 1] += dominantDirection.y;
        lightmap[indexDir * 3 + 2] += dominantDirection.z;
    }
}
