#include "includes/bindings.slang"
#include "includes/core.slang"
#include "includes/sobol_burley.slang"

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
[numthreads(8, 8, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    uint2 resolution;
    VisibilityBuffer.GetDimensions(resolution.x, resolution.y);

    if (coords.x >= resolution.x || coords.y >= resolution.y)
    {
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

    float4 visibility = float4(0.0, 0.0, asfloat(uint::maxValue), 0.0);
    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    Hit hit;
    if (TraceRay(ray, hit))
    {
        if (hit.frontFace)
        {
            visibility.xy = hit.bary;
            visibility.z = asfloat(hit.primitiveID);
            uint pixelSeed = RNGPixelInit(coords, SamplingPattern::BlueNoiseFirst);
            visibility.w = asfloat(pixelSeed);
        }
    }
    else
    {
        color.rgb = SampleSkybox(ray.D);
    }

    LightmapDiffuse[coords] = color;
    VisibilityBuffer[coords] = visibility;
}
