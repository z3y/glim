#include "includes/bake_core.slang"

[[vk::push_constant]]
PushConstants Push;

[vk::constant_id(0)]
const uint USE_CAMERA;

struct PushConstants
{
    Vertex *vertices;
    uint *indices;

    Light *lights;
    uint lightCount;
    uint maxSamples;

    uint sampleIndex;
    uint width;
    uint height;
    uint bounceCount;
};

float Luminance(float3 rgb)
{
    return dot(rgb, float3(0.2126, 0.7152, 0.0722));
}

float3 ShadeLight(Light light, Surface surface, float2 xi)
{
    Ray shadowRay;
    shadowRay.P = RayOffset(surface.P, surface.Ng);
    shadowRay.tMin = 0;

    float falloff;
    float3 L;
    if (light.type == LightType::Directional)
    {
        L = GetDirectionalLightAngle(light.direction, light.ShadowRangeOrAngle, xi);
        falloff = 1.0;

        shadowRay.tMax = RAY_MAX;
    }
    else
    {
        L = RandomPointOnSphere(light.position, light.ShadowRangeOrAngle, xi);

        float3 positionToLight = L - surface.P;
        L = normalize(positionToLight);

        float distanceSquare = dot(positionToLight, positionToLight);

        float rangeSq = light.range * light.range;

        // todo: add falloff type constant
        #if 1
        falloff = GetDefaultUnityAttenuation(distanceSquare, 1.0 / rangeSq);
        #else
        falloff = GetLightFalloff(distanceSquare, 1.0 / rangeSq);
        #endif

        shadowRay.tMax = sqrt(distanceSquare);
    }

    float cosTheta = saturate(dot(surface.N, L));

    shadowRay.D = L;

    float3 color = cosTheta * falloff * light.color;

    [branch]
    if (any(color > 0.0))
    {
        if (TraceShadowRay(shadowRay))
        {
            color = float3(0.0);
        }
    }

    return color;
}

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

    uint encoded = asuint(visibility.a);

    if (encoded == 0)
    {
        return;
    }

    uint primitiveID = asuint(visibility.z);
    float2 bary = visibility.xy;
    bool useSmoothP = (encoded & (1 << 1)) != 0;

    Surface surface = Surface::Get(Push.indices, Push.vertices, primitiveID, bary);

    float3 color = float3(0.0);
    float3 throughput = float3(1.0);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint bounceCount = Push.bounceCount;

    if (USE_CAMERA > 0)
    {
        float3 emission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
        float3 albedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
        color += emission * throughput;
        throughput = albedo;
    }

    if (useSmoothP)
    {
        surface.P = surface.smoothP;
    }

    uint lightCount = Push.lightCount;

    // direct light
    float2 xiLight = OwenHammersley(sampleIndex, sampleCount, 0, coords);

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        color += ShadeLight(Push.lights[lightIndex], surface, xiLight) * throughput;
    }

    float2 xiBounce0 = OwenHammersley(sampleIndex, sampleCount, 1, coords);

    Ray bounceRay;
    bounceRay.P = RayOffset(surface.P, surface.Ng);
    bounceRay.D = RandomDirectionInHemisphere(surface.N, xiBounce0);
    bounceRay.tMin = 0;
    bounceRay.tMax = RAY_MAX;

    // bounces
    for (uint bounceIndex = 0; bounceIndex < bounceCount; bounceIndex++)
    {
        Hit bounceHit;
        if (TraceRay(bounceRay, bounceHit))
        {
            if (!bounceHit.frontFace)
            {
                continue;
                // surfaceBounce.N = -surfaceBounce.N;
                // surfaceBounce.Ng = -surfaceBounce.Ng;
            }

            Surface surfaceBounce = Surface::Get(Push.indices, Push.vertices, bounceHit.primitiveID, bounceHit.bary);

            float3 bounceEmission = SampleEmission(surfaceBounce.uv, surfaceBounce.lightmapGroup).rgb;
            float3 bounceAlbedo = SampleAlbedo(surfaceBounce.uv, surfaceBounce.lightmapGroup).rgb;

            color += bounceEmission * throughput;

            throughput *= bounceAlbedo;

            uint dimOffset = (bounceIndex + 1) * 2;
            float2 xiLight  = OwenHammersley(sampleIndex, sampleCount, dimOffset,     coords);
            float2 xiBounce = OwenHammersley(sampleIndex, sampleCount, dimOffset + 1, coords);

            for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
            {
                color += ShadeLight(Push.lights[lightIndex], surfaceBounce, xiLight) * throughput;
            }

            bounceRay.P = RayOffset(surfaceBounce.P, surfaceBounce.Ng);
            bounceRay.D = RandomDirectionInHemisphere(surfaceBounce.N, xiBounce);
        }
        else
        {
            break;
        }
    }

    AccumulateColor(LightmapDiffuse, coords, sampleIndex, float4(color, 0));
}
