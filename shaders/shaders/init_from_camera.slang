#include "includes/core.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    float3 cameraPosition;
    float fov;

    float3 cameraDirection;
    float pad0;
}

    [shader("compute")]
    [numthreads(8, 8, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    uint2 resolution;
    VisibilityBuffer.GetDimensions(resolution.x, resolution.y);

    if (coords.x >= resolution.x || coords.y >= resolution.y)
    {
        VisibilityBuffer[coords] = EncodeVisibilityBuffer(float2(0, 0), 0, false, false);
        return;
    }

    float2 ndc = (float2(coords) + 0.5) / float2(resolution);
    ndc = ndc * 2.0 - 1.0;
    ndc.y = -ndc.y;

    float aspect = float(resolution.x) / float(resolution.y);
    ndc.x *= aspect * Push.fov;
    ndc.y *= Push.fov;

    float3 worldUp = float3(0.0, 1.0, 0.0);
    float3 camForward = Push.cameraDirection;
    float3 camRight = normalize(cross(camForward, worldUp));
    float3 camUp = cross(camRight, camForward);

    Ray ray;
    ray.P = Push.cameraPosition;
    ray.D = normalize(camForward + ndc.x * camRight + ndc.y * camUp);
    ray.tMin = 0.001;
    ray.tMax = RAY_MAX;

    float4 result;
    Hit hit;
    if (TraceRay(ray, hit))
    {
        Surface surface = Surface::Get(hit.primitiveID, hit.bary);
        bool backfaceGI = surface.backfaceGI;
        bool smooth = UseSmoothPosition(surface);
        bool visible = backfaceGI || hit.frontFace;
        result = EncodeVisibilityBuffer(hit.bary, hit.primitiveID, visible, smooth);
    }
    else
    {
        result = EncodeVisibilityBuffer(float2(0, 0), 0, false, false);
    }

    VisibilityBuffer[coords] = result;
}
