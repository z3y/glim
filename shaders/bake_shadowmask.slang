#include "includes/core.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint lightsStart;
    uint lightsCount;
    uint compactedCount;
    uint pad0;

    float4 *pixels;
    uint sampleIndex;
    uint sampleCount;
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
    CreateSample(index, sampleIndex, sampleCount, positionWS, normalWS, indexing);

    uint lightsStart = Push.lightsStart;
    uint lightsCount = Push.lightsCount;

    float4 occlusionChannels = float4(0.0, 0.0, 0.0, 0.0);

    for (uint lightIndex = lightsStart; lightIndex < lightsCount; lightIndex++)
    {
        Light light = Lights[lightIndex];

        uint channel = light.OcclusionChannel();
        bool shadows = light.CastShadows();
        if (channel > 3 || !shadows)
        {
            continue;
        }

        float3 L;

        if (ShadowMaskVisibility(light, positionWS, L))
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
