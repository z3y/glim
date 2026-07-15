#include "includes/core.slang"
#include "includes/util.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint2 resolution;
    uint unused;
    uint compactedCount;

    uint lightmapType;
    uint groupIndex;
    uint dilate;
    uint pad2;
};

static const int DILATE_RADIUS = 16;
static const float DILATE_SIGMA = 2.5;

float4 DilateLightmap(int2 coords, uint compactedOffset)
{
    float4 colorSum = 0.0;
    float weightSum = 0.0;

    [loop] for (int dy = -DILATE_RADIUS; dy <= DILATE_RADIUS; dy++)
    {
        [loop] for (int dx = -DILATE_RADIUS; dx <= DILATE_RADIUS; dx++)
        {
            if (dx == 0 && dy == 0)
            {
                continue;
            }

            uint2 newCoords = coords + int2(dx, dy);

            uint compactIndex = GetCompactedIndex(float2(newCoords), Push.groupIndex, false);

            if (compactIndex == uint::maxValue)
            {
                continue;
            }

            compactIndex += compactedOffset;

            float4 c = float4(
                CompactedLightmap[compactIndex * 3 + 0],
                CompactedLightmap[compactIndex * 3 + 1],
                CompactedLightmap[compactIndex * 3 + 2],
                1.0);

            float d2 = float(dx * dx + dy * dy);
            float weight = exp(-d2 / (2.0 * DILATE_SIGMA * DILATE_SIGMA));

            colorSum += c * weight;
            weightSum += weight;
        }
    }

    if (weightSum > 0.0)
    {
        return colorSum / weightSum;
    }

    return 0.0;
}

float3 DilateNormal(int2 coords)
{
    float3 colorSum = 0.0;
    float weightSum = 0.0;

    [loop] for (int dy = -DILATE_RADIUS; dy <= DILATE_RADIUS; dy++)
    {
        [loop] for (int dx = -DILATE_RADIUS; dx <= DILATE_RADIUS; dx++)
        {
            if (dx == 0 && dy == 0)
            {
                continue;
            }

            uint2 newCoords = coords + int2(dx, dy);

            uint compactIndex =
                GetCompactedIndex(float2(newCoords), Push.groupIndex, false);

            if (compactIndex == uint::maxValue)
            {
                continue;
            }

            float4 visiblity = CompactedVisibility[compactIndex];
            float3 c = UnpackNormalOctahedron(visiblity.w);

            float d2 = float(dx * dx + dy * dy);
            float weight = exp(-d2 / (2.0 * DILATE_SIGMA * DILATE_SIGMA));

            colorSum += c * weight;
            weightSum += weight;
        }
    }

    if (weightSum > 0.0)
    {
        return colorSum / weightSum;
    }

    return 0.0;
}

[numthreads(8, 8, 1)] void main(uint3 threadID : SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    if (coords.x >= Push.resolution.x || coords.y >= Push.resolution.y)
    {
        return;
    }

    uint linearIndex = coords.y * Push.resolution.x + coords.x;
    uint compactIndex = GetCompactedIndex(float2(coords), Push.groupIndex, false);
    bool active = compactIndex != uint::maxValue;

    [branch] if (Push.lightmapType == 2)
    {
        float4 directionalLightmap = DecompactTarget[linearIndex];
        float3 normalWS = normalize(DilateNormal(coords));

        float3 normalizedDirection = directionalLightmap.rgb;
        float NoL = saturate(dot(normalizedDirection, normalWS));

        normalizedDirection.z = -normalizedDirection.z; // unity convention
        DecompactTarget[linearIndex] = mad(float4(normalizedDirection, NoL), 0.5, 0.5);
        return;
    }

    if (!active)
    {
        if (Push.dilate > 0)
        {
            float4 dilated = DilateLightmap(coords, 0);

            if (Push.lightmapType == 1)
            {
                float4 dilatedDirectional = DilateLightmap(coords, Push.compactedCount);
                float luminance = Luminance(dilated.rgb);
                float3 normalizedDirection = normalize(dilatedDirectional.xyz / luminance);
                dilated.rgb = normalizedDirection;
                dilated.a = 1.0;
            }

            DecompactTarget[linearIndex] = dilated;
        }
        else
        {
            DecompactTarget[linearIndex] = 0.0;
        }
        return;
    }

    float3 diffuse = float3(
        CompactedLightmap[compactIndex * 3 + 0],
        CompactedLightmap[compactIndex * 3 + 1],
        CompactedLightmap[compactIndex * 3 + 2]);

    if (Push.lightmapType == 0)
    {
        DecompactTarget[linearIndex] = float4(diffuse, 1.0);
    }
    else if (Push.lightmapType == 1)
    {
        uint compactIndex = compactIndex + Push.compactedCount;
        float luminance = Luminance(diffuse);

        float3 directional = float3(
            CompactedLightmap[compactIndex * 3 + 0],
            CompactedLightmap[compactIndex * 3 + 1],
            CompactedLightmap[compactIndex * 3 + 2]);

        float3 normalizedDirection = normalize(directional / luminance);

        DecompactTarget[linearIndex] = float4(normalizedDirection, 1.0);
    }
}
