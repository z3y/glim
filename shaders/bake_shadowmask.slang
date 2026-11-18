#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint lightsStart;
    uint lightsCount;
    uint compactedCount;
    uint pad0;

    float4 *pixels;
    uint sampleIndex;
    uint sampleCount;

    Light *lights;
    uint pad1;
    uint pad2;
};

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.compactedCount)
    {
        return;
    }

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.sampleCount;
    float sampleScale = 1.0 / float(sampleCount);

    float3 positionWS;
    float3 normalWS;
    uint3 indexing;
    ShadowTerminator terminator;
    CreateSample(index, sampleIndex, sampleCount, positionWS, normalWS, indexing, terminator);

    uint lightsStart = Push.lightsStart;
    uint lightsCount = Push.lightsCount;

    float2 xiLight = SobolBurleySample2D(indexing.x, DIMENSION_LIGHT, indexing.y, indexing.z);

    float4 occlusionChannels = float4(0.0, 0.0, 0.0, 0.0);

    Light *lights = Push.lights;
    for (uint lightIndex = lightsStart; lightIndex < lightsCount; lightIndex++)
    {
        Light light = lights[lightIndex];

        uint channel = light.OcclusionChannel();
        bool shadows = light.CastShadows();
        if (channel > 3 || !shadows)
        {
            continue;
        }

        float3 L;
        float2 xi = DecorrelateSample2D(xiLight, lightIndex);

        if (ShadowMaskVisibility(light, positionWS, normalWS, xi, terminator, L))
        {
            float NoL = saturate(dot(normalWS, L));
            if (NoL > 0.0)
            {
                occlusionChannels[channel] = 1.0;
            }
        }
    }

    occlusionChannels *= sampleScale;

    Push.pixels[index] += occlusionChannels;
}
