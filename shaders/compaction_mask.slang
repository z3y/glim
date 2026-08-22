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

groupshared uint SharedMask;

[numthreads(32, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID, uint3 groupID: SV_GroupID, uint groupIndex: SV_GroupIndex)
{
    uint2 resolution = Push.resolution;

    if (groupIndex == 0)
    {
        SharedMask = 0;
    }

    GroupMemoryBarrierWithGroupSync();

    bool visible = false;

    if (threadID.x < resolution.x * resolution.y)
    {
        uint2 coords = MortonDecode(threadID.x);

        float4 visibility = VisibilityBuffer[coords];
        visible = asuint(visibility.x) != uint::maxValue;
    }

    if (visible)
    {
        InterlockedOr(SharedMask, 1u << groupIndex);
    }

    GroupMemoryBarrierWithGroupSync();

    if (groupIndex == 0)
    {
        uint mask = SharedMask;
        CompactionBuffer[groupID.x + Push.offset] = uint2(mask, countbits(mask));
    }
}
