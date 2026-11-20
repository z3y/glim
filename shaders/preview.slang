#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint sampleIndex;
    uint sampleCount;
    uint lightCount;
    uint bounceCount;

    uint2 resolution;
    Light *lights;
};

// RGB emitter seen through the path throughput
float3 PathRadiance(float3 emitter, float3 throughput, float4 spectralThroughput, SpectralSample s)
{
    if (!SpectralEnabled() || !any(emitter > 0.0))
    {
        return emitter * throughput;
    }
    return s.ToRGB(EvalEmissionSpectrum(emitter, s.phase) * spectralThroughput);
}

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    if (any(coords >= Push.resolution))
    {
        return;
    }

    float4 visibility = VisibilityBuffer[coords];
    uint primitiveID = asuint(visibility.z);
    if (primitiveID == uint::maxValue)
    {
        return;
    }

    bool frontFace = (primitiveID & PREVIEW_BACKFACE_BIT) == 0;
    primitiveID &= ~PREVIEW_BACKFACE_BIT;

    float2 bary = visibility.xy;
    Surface surface = Surface::Get(primitiveID, bary, frontFace);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.sampleCount;

    uint pixelSeed = asuint(visibility.w);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, ~0u, sampleCount, SamplingPattern::BlueNoiseFirst);
    uint dimension = 0;

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0, 1.0, 1.0);

    // Spectral path: 4 hero wavelengths per sample, throughput is kept per wavelength
    // so repeated bounces multiply the albedo spectra, emitters are evaluated at the same wavelengths.
    bool spectral = SpectralEnabled();
    SpectralSample spectralSample = SpectralSample::Create(SpectralU(pixelSeed, sampleIndex));
    float4 spectralThroughput = 1.0;

    float3 emission = SampleSurfaceEmission(surface);
    float3 albedo = SampleAlbedo(surface.coords, surface.lightmapGroup).rgb;
    // seen directly, exact in RGB
    color.rgb += emission;
    throughput = albedo;
    if (spectral)
    {
        spectralThroughput = Reflectance::Create(albedo).Eval(spectralSample.phase);
    }

    float3 positionWS = RayOffset(surface.P, surface.Ng);
    float3 normalWS = surface.N;
    ShadowTerminator terminator = ShadowTerminator::Create(primitiveID, bary, surface.Ng, frontFace, surface.shadowTerminator);

    for (uint bounceIndex = 0; bounceIndex < (Push.bounceCount + 1); bounceIndex++)
    {
        float2 xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

        for (uint lightIndex = 0; lightIndex < Push.lightCount; lightIndex++)
        {
            float3 L;
            Light light = Push.lights[lightIndex];
            float2 xi = DecorrelateSample2D(xiLight, lightIndex);
            float3 current;
            if (spectral)
            {
                current = ShadeLightDirectSpectral(light, positionWS, normalWS, xi, spectralSample, spectralThroughput, L, terminator);
            }
            else
            {
                current = ShadeLightDirect(light, positionWS, normalWS, xi, throughput, L, false, terminator);
            }
            float NoL = saturate(dot(L, normalWS));
            color.rgb += current * NoL;
        }

        float2 xiBounce = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

        Ray ray;
        ray.tMin = 0;
        ray.tMax = RAY_MAX;
        ray.P = positionWS;
        ray.D = RandomDirectionInHemisphereCosine(normalWS, xiBounce);

        Hit bounceHit;
        if (TraceRay(ray, bounceHit))
        {
            Surface surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary, bounceHit.frontFace);
            positionWS = RayOffset(surface.P, surface.Ng);
            normalWS = surface.N;
            terminator = ShadowTerminator::Create(bounceHit.primitiveID, bounceHit.bary, surface.Ng, bounceHit.frontFace, surface.shadowTerminator);

            if (!bounceHit.frontFace)
            {
                if (!surface.backfaceGI)
                {
                    break;
                }
            }

            float3 bounceAlbedo = SampleAlbedo(surface.coords, surface.lightmapGroup).rgb;
            if (surface.emissive)
            {
                float3 bounceEmission = SampleSurfaceEmission(surface);
                color.rgb += PathRadiance(bounceEmission, throughput, spectralThroughput, spectralSample);
            }

            throughput *= bounceAlbedo;
            if (spectral)
            {
                spectralThroughput *= Reflectance::Create(bounceAlbedo).Eval(spectralSample.phase);
            }
        }
        else
        {
            color.rgb += PathRadiance(SampleSkybox(ray.D), throughput, spectralThroughput, spectralSample);
            break;
        }
    }

    float4 previous = LightmapDiffuse[coords];
    AccumulateColor(previous, sampleIndex, color);
    previous.a = 1.0;
    LightmapDiffuse[coords] = previous;
}
