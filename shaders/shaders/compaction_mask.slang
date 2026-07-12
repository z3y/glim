#include "includes/core.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint2 resolution;
    uint offset;
    uint pad1;
};

[WaveSize(32)]
    [numthreads(32, 1, 1)] void
    main(uint3 threadID : SV_DispatchThreadID, uint3 groupID : SV_GroupID)
{
    uint2 resolution = Push.resolution;

    bool visible = false;

    if (threadID.x < resolution.x * resolution.y)
    {
        uint2 coords;
        coords.x = threadID.x % resolution.x;
        coords.y = threadID.x / resolution.x;

        uint2 visibility = VisibilityBuffer[coords];
        visible = visibility.x != uint::maxValue;
    }

    uint mask = WaveActiveBallot(visible).x;

    if (WaveIsFirstLane())
    {
        CompactionMaskBuffer[groupID.x + Push.offset] = mask;
    }

    uint activeCount = countbits(mask);
}
