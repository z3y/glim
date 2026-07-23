#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint compactedCount;
    uint sampleIndex;
    uint maxSamples;
    uint lightCount;
};

[shader("compute")]
    [numthreads(64, 1, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
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
    uint lightCount = Push.lightCount;
    float sampleScale = 1.0 / float(sampleCount);

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit1D(index);
    uint sobolIndexMask = ~0u;
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    float2 xiLight = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;
    SHProbeL2 sh = SHProbeL2::new ();

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        Light light = Lights[lightIndex];
        float3 L;
        float3 current = ShadeLightDirect(light, positionWS, normalWS, xiLight, throughput, L, false);
        float3 NoL = saturate(dot(L, normalWS));
        color.rgb += current * NoL;
        dominantDirection.xyz += L * Luminance(current * NoL);

        AccumulateL1Lightmap(sh, current, L, normalWS);
    }

    float2 xiBounce = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z);

    Ray ray;
    ray.P = positionWS;
    ray.D = RandomDirectionInHemisphere(normalWS, xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    float randomEmissiveTrianglePdf = 1.0 / float(EMISSIVE_TRIANGLES_COUNT);
    float bsdfPdf = 1.0 / float.getPi();
    float triangleLightPdf = 0.0;
    float3 triangleLight = 0.0;

    if (EMISSIVE_TRIANGLES_COUNT > 0)
    {
        float2 xiTriangle = SobolBurleySample2D(indexing.x, 2, indexing.y, indexing.z);
        uint triIndex = min(uint(xiTriangle.x * float(EMISSIVE_TRIANGLES_COUNT)), EMISSIVE_TRIANGLES_COUNT - 1);
        uint emissivePrimitiveID = EmissiveTriangles[triIndex];
        float3 L;
        triangleLight = ShadeTriangleLight(emissivePrimitiveID, positionWS, normalWS, xiLight, throughput, randomEmissiveTrianglePdf, triangleLightPdf, L);

        float3 emissive = triangleLight * MISWeight(triangleLightPdf, bsdfPdf);
        color.rgb += emissive;
        dominantDirection.xyz += L * Luminance(emissive);
        // AccumulateL2(sh, Push.sampleIndex, emissive, L); // todo triangle sh direct
        // AccumulateL1Lightmap(sh, current, L, normalWS);
    }

    Hit hit;
    if (TraceRay(ray, hit))
    {
        Surface surface = Surface::Get(hit.primitiveID, hit.bary, hit.frontFace);

        if (!hit.frontFace)
        {
            if (!surface.backfaceGI)
            {
                color.a = 0;
            }
        }

        float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
        float3 emission = bounceEmission * color.a;

        if (any(emission > 0.0))
        {
            bool isEmissiveTriange = surface.emissive;
            if (EMISSIVE_TRIANGLES_COUNT > 0 && isEmissiveTriange)
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
                    AccumulateL1Lightmap(sh, emission * 2, ray.D, normalWS);
                }
            }
            else
            {
                color.rgb += emission;
                dominantDirection.xyz += ray.D * Luminance(emission);
                AccumulateL1Lightmap(sh, emission * 2, ray.D, normalWS);
            }
        }
    }
    else
    {
        float3 current = SampleSkybox(ray.D);
        color.rgb += current;
        dominantDirection.xyz += ray.D * Luminance(current);
        AccumulateL1Lightmap(sh, current * 2, ray.D, normalWS);
    }

    ValidateSampleColor(color);

    float4 previous = float4(
        CompactedLightmap[index * 3 + 0],
        CompactedLightmap[index * 3 + 1],
        CompactedLightmap[index * 3 + 2],
        1.0);

    AccumulateColor(previous, sampleIndex, color);
    CompactedLightmap[index * 3 + 0] = previous.x;
    CompactedLightmap[index * 3 + 1] = previous.y;
    CompactedLightmap[index * 3 + 2] = previous.z;

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
