#include "includes/bindings.slang"
#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    float4 *visibility;
    uint compactedCount;
    uint leakDetection;
}

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

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.compactedCount)
    {
        return;
    }

    float4 visibility = Push.visibility[index];

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
    float bestT = float ::maxValue;
    uint bestPrim = 0;
    float2 bestBary = 0;
    if (Push.leakDetection > 0)
    {
        [loop]
        for (int i = 0; i < 4; i++)
        {
            ray.D = rayDirs[i];

            if (TraceRay(ray, hit))
            {
                Surface hitSurf = Surface::Get(hit.primitiveID, hit.bary, true);

                if (hitSurf.backfaceGI)
                {
                    continue;
                }

                float travelUV = MaxInsideTravel(bary, baryDirs[i], uvReach);
                float travelWS = travelUV * worldPerUV[i];
                float eps = 0.05 * worldPerUV[i] / float(resolution); // ~5% of a texel

                bool coplanar = dot(hitSurf.Ng, surface.Ng) > 0.999;

                if (hit.frontFace)
                {
                    if (!coplanar && hit.t < travelWS - eps)
                    {
                        canJitter = false;
                    }
                }
                else
                {
                    if (!hit.frontFace && hit.t < bestT)
                    {
                        moved = true;
                        bestT = hit.t;
                        bestBary = hit.bary;
                        bestPrim = hit.primitiveID;
                    }
                }
            }
        }
    }

    if (moved)
    {
        Surface hitSurf = Surface::Get(bestPrim, bestBary, true);
        positionWS = RayOffset(hitSurf.P, hitSurf.Ng);
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
    Push.visibility[index] = visibility;
}
