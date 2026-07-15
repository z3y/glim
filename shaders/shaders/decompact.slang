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
        float4 value = Push.lightmapType == 2 ? float4(0.5, 0.5, 0.5, 0.0) : 0.0;
        DecompactTarget[linearIndex] = value;
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

        float4 value = float4(normalizedDirection, 1.0);
        if (luminance == 0)
        {
            value = 0.0;
        }
        DecompactTarget[linearIndex] = value;
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
