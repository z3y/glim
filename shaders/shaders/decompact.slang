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

[numthreads(8, 8, 1)] void main(uint3 threadID : SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    if (coords.x >= Push.resolution.x || coords.y >= Push.resolution.y)
    {
        return;
    }

    uint index = InterleaveWithZero(coords.x) |
                 (InterleaveWithZero(coords.y) << 1);

    uint word = index >> 5;
    uint bit = index & 31;

    uint2 entry = CompactionBuffer[Push.offset + word];

    uint linearIndex = coords.y * Push.resolution.x + coords.x;

    if ((entry.x & (1u << bit)) == 0)
    {
        DecompactTarget[linearIndex] = 0.0;
        return;
    }

    uint rank = countbits(entry.x & ((1u << bit) - 1));
    uint compactIndex = entry.y + rank;

    DecompactTarget[linearIndex] = 1.0;
}
