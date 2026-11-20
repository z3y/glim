#include "includes/bindings.slang"
#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    float3 cameraPosition;
    float fov; // tan(fov / 2), or the half view height when orthographic
    float3 cameraDirection;
    uint orthographic;
    float3 cameraUp;
    float pad0;

    uint2 resolution;
    float pad1;
    float pad2;
}

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    uint2 resolution = Push.resolution;

    if (any(coords >= resolution))
    {
        return;
    }

    float2 ndc = (float2(coords) + 0.5) / float2(resolution);
    ndc = ndc * 2.0 - 1.0;
    ndc.y = -ndc.y;

    float aspect = float(resolution.x) / float(resolution.y);
    ndc.x *= aspect * Push.fov;
    ndc.y *= Push.fov;

    float3 camForward = Push.cameraDirection;
    float3 camUp = Push.cameraUp;
    float3 camRight = cross(camForward, camUp);

    Ray ray;
    if (Push.orthographic != 0)
    {
        ray.P = Push.cameraPosition + ndc.x * camRight + ndc.y * camUp;
        ray.D = camForward;
    }
    else
    {
        ray.P = Push.cameraPosition;
        ray.D = normalize(camForward + ndc.x * camRight + ndc.y * camUp);
    }
    ray.tMin = 0.001;
    ray.tMax = RAY_MAX;

    float4 visibility = float4(0.0, 0.0, asfloat(uint::maxValue), 0.0);
    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    Hit hit;
    if (TraceRayAlphaTested(ray, hit))
    {
        bool backfaceGI = !hit.frontFace && Surface::Get(hit.primitiveID, hit.bary, false).backfaceGI;
        if (hit.frontFace || backfaceGI)
        {
            uint primitiveID = hit.primitiveID;
            if (!hit.frontFace)
            {
                primitiveID |= PREVIEW_BACKFACE_BIT;
            }
            visibility.xy = hit.bary;
            visibility.z = asfloat(primitiveID);
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
