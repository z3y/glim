#include "includes/bake_core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

[vk::constant_id(0)] const uint USE_CAMERA;

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

void ShadeLight(inout Spectrum spectrum, Light light, Surface surface, float2 xi, float3 throughput)
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

    float3 color = cosTheta * falloff * light.color * throughput;

    [branch] if (any(color > 0.0))
    {
        if (!TraceShadowRay(shadowRay))
        {
            SpectrumAdd(spectrum, color, L);
        }
    }
}

[shader("compute")]
    [numthreads(8, 8, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
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

    Spectrum spectrum = SpectrumZero();

    Surface surface = Surface::Get(Push.indices, Push.vertices, primitiveID, bary);

    float3 throughput = float3(1.0);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint bounceCount = Push.bounceCount;

    uint pixelSeed = HashPcg3d(coords.xyy).x;
    // todo set constant
    uint shuffledIndexMask = reversebits(NextPowerOfTwo(sampleCount - 1) - 1);
    // uint shuffledIndexMask = ~0u;

    if (USE_CAMERA > 0)
    {
        float3 emission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
        float3 albedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
        spectrum.color += emission;
        throughput = albedo;
    }

    if (useSmoothP)
    {
        surface.P = surface.smoothP;
    }

    uint lightCount = Push.lightCount;

    // direct light
    uint dimension = 0;
    float2 xiLight = SobolBurleySample2D(sampleIndex, dimension++, pixelSeed, shuffledIndexMask);

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        ShadeLight(spectrum, Push.lights[lightIndex], surface, xiLight, throughput);
    }

    float2 xiBounce0 = SobolBurleySample2D(sampleIndex, dimension++, pixelSeed, shuffledIndexMask);

    Ray bounceRay;
    bounceRay.P = RayOffset(surface.P, surface.Ng);
    bounceRay.D = RandomDirectionInHemisphere(surface.N, xiBounce0);
    bounceRay.tMin = 0;
    bounceRay.tMax = RAY_MAX;

    // todo backface gi
    bool backfaceGI = false;

    // todo:
    // fix emission and light bounce count mismatch
    // emissions have bounceCount-1 bounces

    // bounces
    for (uint bounceIndex = 0; bounceIndex < bounceCount; bounceIndex++)
    {
        Hit bounceHit;
        if (TraceRay(bounceRay, bounceHit))
        {
            Surface surfaceBounce = Surface::Get(Push.indices, Push.vertices, bounceHit.primitiveID, bounceHit.bary);

            if (!bounceHit.frontFace)
            {
                if (!backfaceGI)
                {
                    break;
                }

                surfaceBounce.N = -surfaceBounce.N;
                surfaceBounce.Ng = -surfaceBounce.Ng;
            }

            float3 bounceEmission = SampleEmission(surfaceBounce.uv, surfaceBounce.lightmapGroup).rgb;
            float3 bounceAlbedo = SampleAlbedo(surfaceBounce.uv, surfaceBounce.lightmapGroup).rgb;

            SpectrumAdd(spectrum, bounceEmission * throughput, bounceRay.D);

            throughput *= bounceAlbedo;

            float2 xiLight = SobolBurleySample2D(sampleIndex, dimension++, pixelSeed, shuffledIndexMask);
            float2 xiBounce = SobolBurleySample2D(sampleIndex, dimension++, pixelSeed, shuffledIndexMask);

            for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
            {
                ShadeLight(spectrum, Push.lights[lightIndex], surfaceBounce, xiLight, throughput);
            }

            bounceRay.P = RayOffset(surfaceBounce.P, surfaceBounce.Ng);
            bounceRay.D = RandomDirectionInHemisphere(surfaceBounce.N, xiBounce);
        }
        else
        {
            break;
        }
    }

    AccumulateColor(LightmapDiffuse, coords, sampleIndex, float4(spectrum.color, 0));
}
