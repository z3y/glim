[[vk::push_constant]]
PushConstants Push;
struct PushConstants
{
    uint2 resolution;
    uint pad0;
    uint pad1;
};

[[vk::binding(0)]] RWStructuredBuffer<float4> Tex;

#define TILE 8
#define RADIUS 6
#define HALO (TILE + 2 * RADIUS)

groupshared float4 Cache[HALO * HALO];

uint IndexFromCoords(uint2 coords)
{
    return coords.y * Push.resolution.x + coords.x;
}

int2 ClampToRes(int2 c)
{
    return clamp(c, int2(0, 0), int2(Push.resolution) - 1);
}

[shader("compute")]
    [numthreads(TILE, TILE, 1)] void
    main(uint3 groupID : SV_GroupID, uint3 localID : SV_GroupThreadID, uint3 threadID : SV_DispatchThreadID)
{
    int2 tileOrigin = int2(groupID.xy) * TILE - RADIUS;

    uint threadsPerGroup = TILE * TILE;
    uint cacheCells = HALO * HALO;
    for (uint i = localID.y * TILE + localID.x; i < cacheCells; i += threadsPerGroup)
    {
        int2 local = int2(i % HALO, i / HALO);
        int2 global = ClampToRes(tileOrigin + local);
        Cache[i] = Tex[IndexFromCoords(uint2(global))];
    }
    GroupMemoryBarrierWithGroupSync();

    uint2 coords = threadID.xy;
    if (coords.x >= Push.resolution.x || coords.y >= Push.resolution.y)
    {
        return;
    }

    int2 localCoord = int2(localID.xy) + RADIUS;
    float4 center = Cache[localCoord.y * HALO + localCoord.x];
    if (center.a > 0.0)
    {
        return;
    }

    float bestDist2 = 1e9;
    [loop] for (int dy = -RADIUS; dy <= RADIUS; dy++)
    {
        [loop] for (int dx = -RADIUS; dx <= RADIUS; dx++)
        {
            if (dx == 0 && dy == 0)
                continue;
            int2 sampleLocal = localCoord + int2(dx, dy);
            if (sampleLocal.x < 0 || sampleLocal.y < 0 || sampleLocal.x >= HALO || sampleLocal.y >= HALO)
            {
                continue;
            }
            float4 s = Cache[sampleLocal.y * HALO + sampleLocal.x];
            if (s.a > 0.0)
            {
                float d2 = float(dx * dx + dy * dy);
                bestDist2 = min(bestDist2, d2);
            }
        }
    }

    if (bestDist2 >= 1e9)
    {
        return;
    }

    float bestDist = sqrt(bestDist2);
    float band = bestDist + 1.5;

    float4 accumColor = float4(0, 0, 0, 0);
    float accumWeight = 0.0;

    [loop] for (int dy2 = -RADIUS; dy2 <= RADIUS; dy2++)
    {
        [loop] for (int dx2 = -RADIUS; dx2 <= RADIUS; dx2++)
        {
            if (dx2 == 0 && dy2 == 0)
                continue;
            int2 sampleLocal = localCoord + int2(dx2, dy2);
            if (sampleLocal.x < 0 || sampleLocal.y < 0 || sampleLocal.x >= HALO || sampleLocal.y >= HALO)
            {
                continue;
            }
            float4 s = Cache[sampleLocal.y * HALO + sampleLocal.x];
            if (s.a <= 0.0)
                continue;

            float dist = length(float2(dx2, dy2));
            if (dist > band)
                continue;

            float w = 1.0 / (dist * dist + 0.25);
            accumColor += s.rgba * w;
            accumWeight += w;
        }
    }

    float4 result = accumWeight > 0.0 ? accumColor / accumWeight : float4(0, 0, 0, 0);
    Tex[IndexFromCoords(coords)] = result;
}
