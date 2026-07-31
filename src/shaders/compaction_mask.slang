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

[WaveSize(32)]
    [numthreads(32, 1, 1)] void
    main(uint3 threadID : SV_DispatchThreadID, uint3 groupID : SV_GroupID)
{
    uint2 resolution = Push.resolution;

    bool visible = false;

    if (threadID.x < resolution.x * resolution.y)
    {
        uint2 coords = IndexToUV(threadID.x);

        float4 visibility = VisibilityBuffer[coords];
        visible = asuint(visibility.x) != uint::maxValue;
    }

    uint mask = WaveActiveBallot(visible).x;

    if (WaveIsFirstLane())
    {
        CompactionBuffer[groupID.x + Push.offset] = uint2(mask, countbits(mask));
    }
}
