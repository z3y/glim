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
        falloff = GetLightFalloff(distanceSquare, 1.0 / rangeSq);

        shadowRay.tMax = sqrt(distanceSquare);
    }

    float cosTheta = saturate(dot(surface.N, L));

    shadowRay.D = L;

    float3 color = cosTheta * falloff * light.color;

    if (cosTheta > 0.0)
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

    float2 hammersley = Hammersley(sampleIndex, sampleCount);

    if (USE_CAMERA > 0)
    {
        float3 albedo = Albedo[uint2(surface.uv * (float2)resolution)].rgb;
        throughput = albedo;
    }

    if (useSmoothP)
    {
        surface.P = surface.smoothP;
    }

    uint lightCount = Push.lightCount;

    // direct light
    float2 xiLight = Scramble(hammersley, coords, 0, sampleIndex);
    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        color += ShadeLight(Push.lights[lightIndex], surface, xiLight) * throughput;
    }

    float2 xiBounce0 = Scramble(hammersley, coords, 1, sampleIndex);

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
            Surface surfaceBounce = Surface::Get(Push.indices, Push.vertices, bounceHit.primitiveID, bounceHit.bary);

            bool isBackface = dot(surfaceBounce.Ng, bounceRay.D) > 0;

            if (isBackface)
            {
                surfaceBounce.N = -surfaceBounce.N;
                surfaceBounce.Ng = -surfaceBounce.Ng;
            }

            float2 uv = surfaceBounce.uv;
            float3 bounceAlbedo = Albedo[uint2(uv * (float2)resolution)].rgb;

            throughput *= bounceAlbedo;

            uint dimOffset = (bounceIndex + 1) * 2;
            float2 xiLight = Scramble(hammersley, coords, dimOffset, sampleIndex);

            for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
            {
                color += ShadeLight(Push.lights[lightIndex], surfaceBounce, xiLight) * throughput;
            }

            float2 xiBounce = Scramble(hammersley, coords, dimOffset + 1, sampleIndex);

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
