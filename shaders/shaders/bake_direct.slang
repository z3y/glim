#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint width;
    uint height;
    uint sampleIndex;
    uint maxSamples;

    uint lightCount;
    uint pad0;
    uint pad1;
    uint pad2;
};

void ShadeLightDirect(inout Spectrum spectrum, Light light, Surface surface, float2 xi, float3 throughput)
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
    uint lightCount = Push.lightCount;

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit(coords, pattern);
    uint sobolIndexMask = ~0u;
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    // todo temp
    if (sampleIndex == 0)
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

    if (useSmoothP)
    {
        surface.P = surface.smoothP;
    }

    float2 xiLight = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);

    Spectrum spectrum = SpectrumZero();
    float3 throughput = float3(1.0);

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        ShadeLightDirect(spectrum, Lights[lightIndex], surface, xiLight, throughput);
    }

    AccumulateColor(LightmapDiffuse, coords, sampleIndex, float4(spectrum.color, 1.0));
}
