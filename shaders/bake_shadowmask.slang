#include "includes/core.slang"

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

bool ShadowMaskVisibility(Light light, float3 positionWS, out float3 L)
{
    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;

    LightType ty = light.Type();

    if (ty == LightType::Directional)
    {
        shadowRay.D = light.direction;
        shadowRay.tMax = RAY_MAX;

        L = light.position;
    }
    else
    {
        float3 positionToLight = light.position - positionWS;
        float distanceSquare = dot(positionToLight, positionToLight);

        L = normalize(positionToLight);

        if (distanceSquare > (light.range * light.range))
        {
            return false;
        }

        if (ty == LightType::Spot)
        {
            float spotScale;
            float spotOffset;
            SpotLightScaleOffset(light.SpotOuter + 1, light.SpotInnerPercent, spotScale, spotOffset);

            if (SpotLightAngleAttenuation(light.direction, L, spotScale, spotOffset) <= 0.0)
            {
                return false;
            }
        }

        shadowRay.D = L;
        shadowRay.tMax = sqrt(distanceSquare);
    }

    return !TraceShadowRay(shadowRay);
}

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
