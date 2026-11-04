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
};

float MaxInsideTravel(float2 bary, float2 dir, float maxS)
{
    float s = maxS;
    if (dir.x < 0.0)
        s = min(s, -bary.x / dir.x); // b.x > 0
    if (dir.y < 0.0)
        s = min(s, -bary.y / dir.y); // b.y > 0
    float sum = dir.x + dir.y;
    if (sum > 0.0)
        s = min(s, (1.0 - bary.x - bary.y) / sum); // b.x + b.y < 1
    return max(s, 0.0);
}

void InitSamples(uint index)
{
    float4 visibility = CompactedVisibility[index];

    float2 bary = visibility.xy;
    uint primitiveID = asuint(visibility.z);

    Vertex[3] vert = TriangleVertices(primitiveID);

    float3 p0 = vert[0].position;
    float3 p1 = vert[1].position;
    float3 p2 = vert[2].position;
    float2 uv0 = vert[0].uv;
    float2 uv1 = vert[1].uv;
    float2 uv2 = vert[2].uv;
    uint resolution = vert[0].Resolution();

    float3 edge1 = p1 - p0;
    float3 edge2 = p2 - p0;
    float2 duv1 = uv1 - uv0;
    float2 duv2 = uv2 - uv0;

    float det = duv1.x * duv2.y - duv1.y * duv2.x;
    float invDetSigned = abs(det) > 1e-12 ? 1.0 / det : 0.0;

    Surface surface = Surface::Get(primitiveID, bary);

    float3 positionWS = RayOffset(surface.P, surface.Ng);
    float3 normalWS = surface.N;

    float3 dPdU = (edge1 * duv2.y - edge2 * duv1.y) * invDetSigned;
    float3 dPdV = (edge2 * duv1.x - edge1 * duv2.x) * invDetSigned;

    float3 u = normalize(dPdU - dot(dPdU, surface.Ng) * surface.Ng);
    float3 v = normalize(dPdV - dot(dPdV, surface.Ng) * surface.Ng);
    float3 rayDirs[4] = { u, -u, v, -v };

    float texelSize = 1.0 / float(resolution);
    float3 worldTexelU = dPdU * texelSize;
    float3 worldTexelV = dPdV * texelSize;

    float rayLength = (length(worldTexelU) + length(worldTexelV)) * 2.0;

    float2 baryPerU = float2(duv2.y, -duv1.y) * invDetSigned; // jitterUV = (1, 0)
    float2 baryPerV = float2(-duv2.x, duv1.x) * invDetSigned; // jitterUV = (0, 1)

    float2 baryDirs[4] = { baryPerU, -baryPerU, baryPerV, -baryPerV };
    float worldPerUV[4] = { length(dPdU), length(dPdU), length(dPdV), length(dPdV) };

    float uvReach = 0.5 / float(resolution); // max jitter per axis, in UV

    Ray ray;
    ray.P = positionWS;
    ray.tMin = 0;
    ray.tMax = rayLength;
    Hit hit;

    bool canJitter = true;
    bool moved = false;
    [loop]
    for (int i = 0; i < 4; i++)
    {
        ray.D = rayDirs[i];

        if (TraceRay(ray, hit))
        {
            float travelUV = MaxInsideTravel(bary, baryDirs[i], uvReach);
            float travelWS = travelUV * worldPerUV[i];
            float eps = 0.05 * worldPerUV[i] / float(resolution); // ~5% of a texel

            Surface hitSurf = Surface::Get(hit.primitiveID, hit.bary, true);
            bool coplanar = dot(hitSurf.Ng, surface.Ng) > 0.999;

            if (!coplanar && hit.t < travelWS - eps)
            {
                canJitter = false;
            }

            if (!hit.frontFace)
            {
                if (!hitSurf.backfaceGI)
                {
                    positionWS = RayOffset(hitSurf.P, hitSurf.Ng);
                    moved = true;
                    break;
                }
            }
        }
    }

    if (moved || !canJitter || JITTER_BARYCENTRICS == 0)
    {
        visibility.xyz = positionWS;
        visibility.w = PackNormalOctahedron(normalWS);
    }
    else
    {
        visibility.w = asfloat(0u);
    }
    CompactedVisibility[index] = visibility;
}

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.compactedCount)
    {
        return;
    }

    [branch]
    if (Push.sampleIndex == 0)
    {
        InitSamples(index);
    }

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    float sampleScale = 1.0 / float(sampleCount);

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
            float3 bounceEmission = SampleEmission(surface.coords, surface.lightmapGroup).rgb;
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

    // if (asuint(CompactedVisibility[index].w) != 0u)
    // {
    //     color.rgb = float3(1, 0, 0);
    // }

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
