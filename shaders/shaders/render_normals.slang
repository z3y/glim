#include "includes/core.slang"

[shader("compute")]
    [numthreads(8, 8, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    uint2 resolution;
    VisibilityBuffer.GetDimensions(resolution.x, resolution.y);

    if (coords.x >= resolution.x || coords.y >= resolution.y)
    {
        return;
    }

    float4 visibility = VisibilityBuffer[coords];

    uint encoded = asuint(visibility.a);

    if (encoded == 0)
    {
        NormalsBuffer[coords] = float4(0, 0, 0, 0);
        return;
    }

    uint primitiveID = asuint(visibility.z);
    float2 bary = visibility.xy;
    Surface surface = Surface::Get(primitiveID, bary);

    NormalsBuffer[coords] = float4(surface.N, 1);
}
