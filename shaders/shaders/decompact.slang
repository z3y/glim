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

static const int DILATE_RADIUS = 1;

void DilateLightmap(int2 coords, uint compactedOffset, inout float4 original)
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
            float weight = 1.0 / (d2 * d2 + 1.0);

            colorSum += c * weight;
            weightSum += weight;
        }
    }

    if (weightSum > 0.0)
    {
        original = colorSum / weightSum;
    }
}

float4 EncodeDirectional(float3 directional, float3 diffuse)
{
    float luminance = Luminance(diffuse);
    float3 normalizedDirection = normalize(directional / luminance);

    float4 value = float4(normalizedDirection, 1.0);
    if (luminance == 0.0)
    {
        value = 0.0;
    }

    return value;
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

    if (!active)
    {
        if (Push.lightmapType == 2)
        {
            DecompactTarget[linearIndex] = float4(0.5, 0.5, 0.5, 0.0);
        }
        else
        {
            float4 dilatedDiffuse = 0.0;
            DilateLightmap(coords, 0, dilatedDiffuse);
            if (Push.lightmapType == 1)
            {
                float4 dilatedDirectional = 0.0;
                DilateLightmap(coords, Push.compactedCount, dilatedDirectional);
                float4 encoded = EncodeDirectional(dilatedDirectional.xyz, dilatedDiffuse.rgb);
                DecompactTarget[linearIndex] = float4(encoded.xyz, 0.0);
            }
            else
            {
                DecompactTarget[linearIndex] = float4(dilatedDiffuse.xyz, 0.0);
            }
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

        float3 directional = float3(
            CompactedLightmap[compactIndex * 3 + 0],
            CompactedLightmap[compactIndex * 3 + 1],
            CompactedLightmap[compactIndex * 3 + 2]);

        DecompactTarget[linearIndex] = EncodeDirectional(directional, diffuse);
    }
    else if (Push.lightmapType == 2)
    {
        float4 directionalLightmap = DecompactTarget[linearIndex];

        float4 visiblity = CompactedVisibility[compactIndex];
        float3 normalWS = UnpackNormalOctahedron(visiblity.w);

        float3 normalizedDirection = directionalLightmap.rgb;
        float NoL = saturate(dot(normalizedDirection, normalWS));

        normalizedDirection.z = -normalizedDirection.z; // unity convention

        float4 encoded = mad(float4(normalizedDirection, NoL), 0.5, 0.5);
        DecompactTarget[linearIndex] = encoded;
    }
}
