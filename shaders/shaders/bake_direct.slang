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

float2 BaryFromUV(uint primitiveID, float2 uv)
{
    uint i0 = Indices[primitiveID * 3 + 0];
    uint i1 = Indices[primitiveID * 3 + 1];
    uint i2 = Indices[primitiveID * 3 + 2];

    Vertex v0 = Vertices[i0];
    Vertex v1 = Vertices[i1];
    Vertex v2 = Vertices[i2];

    float2 uv0 = v0.GetUV();
    float2 uv1 = v1.GetUV();
    float2 uv2 = v2.GetUV();

    float2 e0 = uv1 - uv0;
    float2 e1 = uv2 - uv0;
    float2 p = uv - uv0;

    float det = e0.x * e1.y - e0.y * e1.x;

    if (abs(det) < 1e-8)
    {
        return float2(0.0, 0.0);
    }

    float invDet = 1.0 / det;

    float b1 = (p.x * e1.y - p.y * e1.x) * invDet;
    float b2 = (-p.x * e0.y + p.y * e0.x) * invDet;
    float b0 = 1.0 - b1 - b2;

    return float2(b1, b2);
}

[shader("compute")]
    [numthreads(64, 1, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.compactedCount)
    {
        return;
    }

    uint2 visibility = CompactedVisibility[index];
    uint primitiveID = visibility.x;
    float2 uv = float2(UnpackCoords(visibility.y)) + 0.5;
    float2 bary = BaryFromUV(primitiveID, uv);

    Surface surface = Surface::Get(primitiveID, bary, true);

    CompactedDiffuse[index] = surface.Ng;

    // float3 positionWS = visibility.xyz;
    // float3 normalWS = UnpackNormalOctahedron(visibility.w);

    // uint sampleIndex = Push.sampleIndex;
    // uint sampleCount = Push.maxSamples;
    // uint lightCount = Push.lightCount;

    // SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    // uint pixelSeed = RNGPixelInit(coords, pattern);
    // uint sobolIndexMask = ~0u;
    // uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    // float2 xiLight = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);

    // float4 color = float4(0.0, 0.0, 0.0, 1.0);
    // float3 throughput = float3(1.0);
    // float4 dominantDirection = 0.0;

    // for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    // {
    //     float3 L;
    //     float3 current = ShadeLightDirect(Lights[lightIndex], positionWS, normalWS, xiLight, throughput, L);
    //     color.rgb += current;
    //     dominantDirection.xyz += L * Luminance(current);
    // }

    // float2 xiBounce = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z);

    // Ray ray;
    // ray.P = positionWS;
    // ray.D = RandomDirectionInHemisphere(normalWS, xiBounce);
    // ray.tMin = 0;
    // ray.tMax = RAY_MAX;

    // Hit bounceHit;
    // if (TraceRay(ray, bounceHit))
    // {
    //     Surface surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary, bounceHit.frontFace);

    //     if (!bounceHit.frontFace)
    //     {
    //         if (!surface.backfaceGI)
    //         {
    //             color.a = 0;
    //         }
    //     }

    //     float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
    //     float3 current = bounceEmission * color.a;
    //     color.rgb += bounceEmission * color.a;
    //     dominantDirection.xyz += ray.D * Luminance(current);
    // }

    // float4 previous = LightmapDiffuse[coords];
    // color.a = 1.0;
    // AccumulateColor(previous, sampleIndex, color);
    // LightmapDiffuse[coords] = previous;

    // uint index = (coords.x + coords.y * resolution.x) * 4;
    // dominantDirection.z = -dominantDirection.z; // unity coords

    // // todo temp
    // if (sampleIndex == 0)
    // {
    //     DominantDirection[index + 0] = dominantDirection.x;
    //     DominantDirection[index + 1] = dominantDirection.y;
    //     DominantDirection[index + 2] = dominantDirection.z;
    //     DominantDirection[index + 3] = PackNormalOctahedron(float3(normalWS.xy, -normalWS.z));
    // }
    // else
    // {
    //     DominantDirection[index + 0] += dominantDirection.x;
    //     DominantDirection[index + 1] += dominantDirection.y;
    //     DominantDirection[index + 2] += dominantDirection.z;
    // }
}
