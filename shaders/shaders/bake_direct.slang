#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint width;
    uint height;
    uint sampleIndex;
    uint maxSamples;

    uint lightCount;
    uint pad0;
    uint pad1;
    uint pad2;
};

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;
    uint2 resolution = uint2(Push.width, Push.height);

    if (coords.x >= resolution.x || coords.y >= resolution.y)
    {
        return;
    }

    // float4 visibility = VisibilityBuffer[coords];

    // uint encoded = asuint(visibility.a);

    // if (encoded == 0)
    // {
    //     return;
    // }

    // uint primitiveID = asuint(visibility.z);
    // float2 bary = visibility.xy;
    // bool useSmoothP = (encoded & (1 << 1)) != 0;

    // Surface surface = Surface::Get(primitiveID, bary);

    // uint sampleIndex = Push.sampleIndex;
    // uint sampleCount = Push.maxSamples;
    // uint lightCount = Push.lightCount;

    // SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    // uint pixelSeed = RNGPixelInit(coords, pattern);
    // uint sobolIndexMask = ~0u;
    // uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    // // todo temp
    // if (sampleIndex == 0)
    // {
    //     visibility = AdjustSamplePositions(surface, visibility);
    //     VisibilityBuffer[coords] = visibility;
    //     encoded = asuint(visibility.a);

    //     if (encoded == 0)
    //     {
    //         return;
    //     }

    //     primitiveID = asuint(visibility.z);
    //     bary = visibility.xy;
    //     useSmoothP = (encoded & (1 << 1)) != 0;
    // }

    // if (useSmoothP)
    // {
    //     surface.P = surface.smoothP;
    // }

    // float2 xiLight = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);

    // float4 color = 0.0;
    // float3 throughput = float3(1.0);

    // for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    // {
    //     float3 L;
    //     color.rgb += ShadeLightDirect(Lights[lightIndex], surface, xiLight, throughput, L);
    // }

    // // uint emissiveTriangleCount = EMISSIVE_TRIANGLES_COUNT;
    // // float randomEmissiveTrianglePdf = 1.0 / emissiveTriangleCount;
    // // float bsdfPdf = 1.0 / float.getPi();

    // // if (MULTIPLE_IMPORTANCE_SAMPLING > 0 && EMISSIVE_TRIANGLES_COUNT > 0)
    // // {
    // //     float triangleLightPdf = 0.0;
    // //     float3 triangleLight = 0.0;

    // //     float2 xiTriangle = SobolBurleySample2D(indexing.x, 2, indexing.y, indexing.z);
    // //     uint emissivePrimitiveID = EmissiveTriangles[uint(xiTriangle.x * (float)emissiveTriangleCount)];
    // //     triangleLight = ShadeTriangleLight(emissivePrimitiveID, surface, xiLight, throughput, randomEmissiveTrianglePdf, triangleLightPdf);

    // //     SpectrumAdd(spectrum, triangleLight * MISWeight(triangleLightPdf, bsdfPdf), 0);
    // // }

    // float2 xiBounce = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z);

    // Ray bounceRay;
    // bounceRay.P = RayOffset(surface.P, surface.Ng);
    // bounceRay.D = RandomDirectionInHemisphere(surface.N, xiBounce);
    // bounceRay.tMin = 0;
    // bounceRay.tMax = RAY_MAX;

    // Hit bounceHit;
    // if (TraceRay(bounceRay, bounceHit))
    // {
    //     Surface previousSurface = surface;
    //     surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary);

    //     if (!bounceHit.frontFace)
    //     {
    //         if (!surface.backfaceGI)
    //         {
    //             return;
    //         }

    //         surface.N = -surface.N;
    //         surface.Ng = -surface.Ng;
    //     }

    //     float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;

    //     if (any(bounceEmission > 0.0))
    //     {
    //         // if (MULTIPLE_IMPORTANCE_SAMPLING > 0 && EMISSIVE_TRIANGLES_COUNT > 0)
    //         // {
    //         //     float triangleArea = CalculateTriangleArea(bounceHit.primitiveID);
    //         //     float3 positionToLight = surface.P - previousSurface.P;
    //         //     float distSq = dot(positionToLight, positionToLight);
    //         //     float cosThetaLight = saturate(dot(-normalize(positionToLight), surface.N));
    //         //     float hypotheticalLightPdf = (randomEmissiveTrianglePdf / triangleArea) * distSq / cosThetaLight;
    //         //     SpectrumAdd(spectrum, bounceEmission * MISWeight(bsdfPdf, hypotheticalLightPdf), 0);
    //         // }
    //         // else
    //         // {
    //         SpectrumAdd(spectrum, bounceEmission, 0);
    //         // }
    //     }
    // }

    // AccumulateColor(LightmapDiffuse, coords, sampleIndex, float4(spectrum.color, 1.0));
}
