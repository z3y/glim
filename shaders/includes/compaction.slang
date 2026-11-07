#pragma once

// struct LightmapInfo
// {
//     uint2 resolution;
//     uint compactionOffset;
//     uint cellTableBase;
// };

uint GetCompactedIndex(uint2 coords, uint lightmapGroup, uint primitiveID, bool flipY = true)
{
    LightmapInfo info = Push.lightmapsInfo[lightmapGroup];

    if (flipY)
    {
        coords.y = info.resolution.y - 1 - coords.y;
    }

    uint index = coords.y * info.resolution.x + coords.x;

    uint word = index >> 5;
    uint bit = index & 31;

    uint2 *compaction = Push.compaction;
    uint2 entry = compaction[info.compactionOffset + word];

    if ((entry.x & (1u << bit)) == 0)
    {
        return uint::maxValue;
    }

    uint rank = countbits(entry.x & ((1u << bit) - 1));
    uint cellRank = entry.y + rank;

    uint2 run = compaction[info.cellTableBase + cellRank];

    // Prefer the texel that belongs to the triangle we actually hit.
    // uint end = run.x + run.y;
    // for (uint i = run.x; i < end; i++)
    // {
    //     float4 visibility = CompactedVisibility[i];
    //     bool moved = asuint(visibility.w) != 0u;

    //     if (!moved && asuint(visibility.z) == primitiveID)
    //     {
    //         return i;
    //     }
    // }
    // first in the run is the best-covered texel
    return run.x;
}
