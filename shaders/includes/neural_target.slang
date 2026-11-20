#pragma once

// Noisy unbiased estimates of the prefiltered incoming radiance the network learns, see neural.slang.
// Needs compaction.slang, core.slang and Push.rt / Push.compaction / Push.lightmapsInfo.

#include "neural.slang"

struct NeuralPixel
{
    uint texelStart;
    uint texelCount;
    uint group;
    uint index; // pixel index in the lightmap of the group
};

struct NeuralTexel
{
    uint index;
    float coverage;
};

// everything the neural shaders need, uploaded once, must match NeuralData in src/neural.rs
struct NeuralData
{
    float *weights;
    float *weightM;
    float *weightV;
    float *partials;

    float *features;
    float *featureM;
    float *featureV;
    uint *visits;

    float *batch;
    float *loss;
    float *eval;
    float *lightmap;

    NeuralPixel *pixels;
    NeuralTexel *texels;
    uint *order;
    Light *lights;

    uint pixelCount;
    uint lightCount;
    uint mixedStart;
    uint mixedEnd;
};

struct NeuralSurface
{
    float3 positionWS;
    float3 normalWS;
    float3 geometricNormalWS;
    ShadowTerminator terminator;
};

NeuralSurface NeuralCreateSurface(NeuralData *data, NeuralPixel pixel, uint sampleIndex, float xiTexel)
{
    // pick one of the texels covering the pixel, weighted by coverage
    float total = 0.0;
    for (uint i = 0; i < pixel.texelCount; i++)
    {
        total += data.texels[pixel.texelStart + i].coverage;
    }

    float target = xiTexel * total;
    uint texelIndex = data.texels[pixel.texelStart].index;
    float sum = 0.0;
    for (uint i = 0; i < pixel.texelCount; i++)
    {
        NeuralTexel t = data.texels[pixel.texelStart + i];
        sum += t.coverage;
        texelIndex = t.index;
        if (target < sum)
        {
            break;
        }
    }

    NeuralSurface s;
    uint3 indexing;
    const uint SAMPLE_COUNT = 1u << 16;
    CreateSample(texelIndex, sampleIndex & (SAMPLE_COUNT - 1), SAMPLE_COUNT, s.positionWS, s.normalWS, s.geometricNormalWS, indexing, s.terminator);
    return s;
}

// radiance arriving at the surface from a direction (bake space)
float3 NeuralIncomingRadiance(NeuralData *data, NeuralSurface surface, float3 dir, inout NNRng rng)
{
    if (!IsAboveSurface(dir, surface.geometricNormalWS))
    {
        return 0.0;
    }

    Ray ray;
    ray.P = surface.positionWS;
    ray.D = dir;
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    Hit hit;
    if (!TraceRay(ray, hit))
    {
        return SampleSkybox(dir);
    }

    Surface s = Surface::Get(hit.primitiveID, hit.bary, hit.frontFace);
    if (!hit.frontFace && !s.backfaceGI)
    {
        return 0.0;
    }

    float3 radiance = 0.0;
    if (s.emissive)
    {
        radiance += SampleSurfaceEmission(s);
    }

    float3 albedo = SampleAlbedo(s.coords, s.lightmapGroup).rgb;

    uint compactIndex = GetCompactedIndex(s.coords, s.lightmapGroup, hit.primitiveID, hit.frontFace);
    if (compactIndex != uint::maxValue)
    {
        float *lightmap = data.lightmap;
        float3 diffuse = float3(lightmap[compactIndex * 3 + 0], lightmap[compactIndex * 3 + 1], lightmap[compactIndex * 3 + 2]);
        if (all(isfinite(diffuse)))
        {
            radiance += diffuse * albedo;
        }
    }

    // direct light of mixed lights is real time, but what they light up is seen in reflections
    if (data.mixedEnd > data.mixedStart)
    {
        float3 positionWS = RayOffset(s.P, s.Ng);
        float3 throughput = 1.0;
        for (uint lightIndex = data.mixedStart; lightIndex < data.mixedEnd; lightIndex++)
        {
            Light light = data.lights[lightIndex];
            if (!light.Mixed())
            {
                continue;
            }
            float3 L;
            float3 current = ShadeLightDirect(light, positionWS, s.Ng, rng.next2(), throughput, L, false, ShadowTerminator::None());
            radiance += current * saturate(dot(L, s.Ng)) * albedo;
        }
    }

    return radiance;
}

// lobe: query direction (bake space), cosine kernel when cosine is true, otherwise a normalized SG with the sharpness
float3 NeuralTarget(NeuralData *data, NeuralSurface surface, float3 direction, bool cosine, float sharpness, uint rays, inout NNRng rng)
{
    float3 result = 0.0;

    // baked lights are never hit by rays, they are sampled directly
    float3 throughput = 1.0;
    for (uint lightIndex = 0; lightIndex < data.lightCount; lightIndex++)
    {
        Light light = data.lights[lightIndex];
        if (light.Mixed())
        {
            continue;
        }

        float3 L;
        float3 current = ShadeLightDirect(light, surface.positionWS, surface.normalWS, rng.next2(), throughput, L, false, surface.terminator);

        // ShadeLightDirect skips the shadow ray below the shading normal
        if (dot(L, surface.normalWS) <= 0.0 || !IsAboveSurface(L, surface.geometricNormalWS))
        {
            continue;
        }

        // current * NoL is the diffuse contribution, the radiance integrated over the light is pi * current
        if (cosine)
        {
            result += current * saturate(dot(direction, L));
        }
        else
        {
            result += current * (float.getPi() * NNSGNormalized(direction, sharpness, L));
        }
    }

    float3 traced = 0.0;
    for (uint i = 0; i < rays; i++)
    {
        float2 xi = rng.next2();
        float3 dir = cosine ? RandomDirectionInHemisphereCosine(direction, xi) : NNSampleSG(direction, sharpness, xi);
        traced += NeuralIncomingRadiance(data, surface, dir, rng);
    }
    result += traced / float(rays);

    return result;
}

void NNBuildInput(float *features, uint pixelIndex, float3 directionBake, float lobeCode, out float x[NN_IN])
{
    [unroll]
    for (uint k = 0; k < NN_FEATURES; k++)
    {
        x[k] = features[pixelIndex * NN_FEATURES + k];
    }

    // flip Z for Unity
    float3 d = directionBake;
    d.z = -d.z;
    NNEncodeDirection(d, x);
    NNEncodeLobe(lobeCode, x);
}
