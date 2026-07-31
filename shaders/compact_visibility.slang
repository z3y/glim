#include "includes/core.slang"
#include "includes/util.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint2 resolution;
    uint offset;
    uint pad0;
};

[numthreads(8, 8, 1)] void
main(uint3 threadID : SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    if (coords.x >= Push.resolution.x || coords.y >= Push.resolution.y)
    {
        return;
    }

    uint index = InterleaveWithZero(coords.x) |
                 (InterleaveWithZero(coords.y) << 1);

    uint word = index >> 5; // /32
    uint bit = index & 31;  // %32

    uint2 entry = CompactionBuffer[Push.offset + word];

    bool active = (entry.x & (1u << bit)) != 0;

    if (active)
    {
        uint rank = countbits(entry.x & ((1u << bit) - 1));
        uint compactIndex = entry.y + rank;

        float4 expandedVisibility = VisibilityBuffer[coords];
        uint primitiveID = asuint(expandedVisibility.x);
        float2 bary = expandedVisibility.yz;
        float3 normal = Surface::Get(primitiveID, bary).N;

        float4 visibility = float4(expandedVisibility.xyz, PackNormalOctahedron(normal));
        CompactedVisibility[compactIndex] = visibility;
    }
}
