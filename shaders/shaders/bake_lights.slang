#include "includes/bake_core.slang"

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;
    uint2 resolution = uint2(Push.width, Push.height);

    if (coords.x >= resolution.x || coords.y >= resolution.y)
    {
        return;
    }

    float4 visibility = VisibilityBuffer[coords];

    if (visibility.a == 0)
    {
        return;
    }

    uint primitiveID = asuint(visibility.z);
    float2 bary = visibility.xy;

    Surface surf = Surface::Get(Push.indices, Push.vertices, primitiveID, bary);

    float4 result = 0;

    result.rgb = surf.P;

    Target0[coords] = float4(result.rgb, 1);

    // InterlockedAdd(Target0[coords].x, 1);
    // InterlockedAdd(Target0[coords].y, 1);
    // InterlockedAdd(Target0[coords].z, 1);
    // InterlockedAdd(Target0[coords].w, 1);
}
