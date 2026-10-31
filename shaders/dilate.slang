#include "includes/core.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint texelCount;
    float threshold;
    float *validity;

    uint normalize;
    float pad0;
    float pad1;
    float pad2;
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

    float validity = Push.validity[index];

    [branch]
    if (Push.normalize > 0)
    {
        if (validity > Push.threshold)
        {
            CompactedLightmap[index * 3 + 0] /= validity;
            CompactedLightmap[index * 3 + 1] /= validity;
            CompactedLightmap[index * 3 + 2] /= validity;
        }
        return;
    }

    [branch]
    if (validity > Push.threshold)
    {
        return;
    }

    float4 visibility = CompactedVisibility[index];
    float2 bary = visibility.xy;
    uint primitiveID = asuint(visibility.z);

    Surface surface = Surface::Get(primitiveID, bary, true);
    LightmapInfo info = LightmapsInfo[surface.lightmapGroup];

    int2 p = int2(surface.uv);
    int res = info.resolution.x;
    int rad = 4;

    int bestRing = 0x7fffffff;
    int bestDist = 0x7fffffff;
    float bestCov = 0.0;
    float3 bestRgb = float3(0, 0, 0);

    int minX = max(p.x - rad, 0);
    int maxX = min(p.x + rad, res - 1);
    int minY = max(p.y - rad, 0);
    int maxY = min(p.y + rad, res - 1);

    for (int sy = minY; sy <= maxY; sy++)
    {
        for (int sx = minX; sx <= maxX; sx++)
        {
            int dx = abs(sx - p.x);
            int dy = abs(sy - p.y);
            int ring = max(dx, dy);

            if (ring > bestRing)
            {
                continue;
            }

            uint compactIndex = GetCompactedIndex(float2(sx, sy), surface.lightmapGroup, primitiveID, true);
            if (compactIndex == uint::maxValue)
            {
                continue;
            }

            float4 s = float4(CompactedLightmap[compactIndex * 3 + 0],
                              CompactedLightmap[compactIndex * 3 + 1],
                              CompactedLightmap[compactIndex * 3 + 2],
                              Push.validity[compactIndex]);

            if (s.a <= Push.threshold)
            {
                continue;
            }

            // already normalized in first pass
            // s.rgb /= s.a;

            int dist = dx * dx + dy * dy;

            bool better = ring < bestRing || s.a > bestCov || (s.a == bestCov && dist < bestDist);

            if (better)
            {
                bestRgb = s.rgb;
                bestRing = ring;
                bestCov = s.a;
                bestDist = dist;
            }
        }
    }

    visibility.w = asfloat(uint::maxValue);
    CompactedVisibility[index] = visibility;
    CompactedLightmap[index * 3 + 0] = bestRgb.r;
    CompactedLightmap[index * 3 + 1] = bestRgb.g;
    CompactedLightmap[index * 3 + 2] = bestRgb.b;
}
