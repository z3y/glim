#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
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

        falloff = LightFalloff(distanceSquare, 1.0 / rangeSq);

        shadowRay.tMax = sqrt(distanceSquare);
    }

    float cosTheta = saturate(dot(surface.N, L));

    shadowRay.D = L;

    float3 color = cosTheta * falloff * light.color * throughput;

    [branch] if (any(color > 0.0))
    {
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    SpectrumAdd(spectrum, color, L);
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

    Spectrum spectrum = SpectrumZero();
    float3 throughput = float3(1.0);

    float4 visibility = VisibilityBuffer[coords];

    uint encoded = asuint(visibility.a);

    if (encoded == 0)
    {
        return;
    }

    uint primitiveID = asuint(visibility.z);
    float2 bary = visibility.xy;
    bool useSmoothP = (encoded & (1 << 1)) != 0;

    Surface surface = Surface::Get(primitiveID, bary);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint bounceCount = Push.bounceCount;
    uint lightCount = Push.lightCount;
    // todo set constant

    // this is actually incorrect in cycles
    // uint sobolIndexMask = reversebits(NextPowerOfTwo(sampleCount - 1) - 1);
    uint sobolIndexMask = NextPowerOfTwo(sampleCount - 1) - 1;

    // todo temp
    if (USE_CAMERA == 0)
    {
        if (Push.sampleIndex == 0)
        {
            visibility = AdjustSamplePositions(surface, visibility);
            VisibilityBuffer[coords] = visibility;
            encoded = asuint(visibility.a);

            if (encoded == 0)
            {
                return;
            }

            primitiveID = asuint(visibility.z);
            bary = visibility.xy;
            useSmoothP = (encoded & (1 << 1)) != 0;
        }
    }

    SamplingPattern pattern = SamplingPattern::BlueNoisePure;
    if (USE_CAMERA > 0)
    {
        pattern = SamplingPattern::BlueNoiseFirst;
    }
    uint pixelSeed = RNGPixelInit(coords, pattern);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);
    uint dimension = 0;

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

    // direct light
    float2 xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        ShadeLight(spectrum, Lights[lightIndex], surface, xiLight, throughput);
    }

    float2 xiBounce0 = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

    Ray bounceRay;
    bounceRay.P = RayOffset(surface.P, surface.Ng);
    bounceRay.D = RandomDirectionInHemisphere(surface.N, xiBounce0);
    bounceRay.tMin = 0;
    bounceRay.tMax = RAY_MAX;

    // todo:
    // fix emission and light bounce count mismatch
    // emissions have bounceCount-1 bounces

    // bounces
    for (uint bounceIndex = 0; bounceIndex < bounceCount; bounceIndex++)
    {
        Hit bounceHit;
        if (TraceRay(bounceRay, bounceHit))
        {
            Surface surfaceBounce = Surface::Get(bounceHit.primitiveID, bounceHit.bary);
            bool backfaceGI = surfaceBounce.backfaceGI;

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

            float2 xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);
            float2 xiBounce = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

            for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
            {
                ShadeLight(spectrum, Lights[lightIndex], surfaceBounce, xiLight, throughput);
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
