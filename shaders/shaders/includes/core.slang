#pragma once

#include "bindings.slang"

static const float RAY_MAX = 10000.0;
#define ALPHA_TEST 1

struct Ray
{
    float3 P;
    float tMin;
    float3 D;
    float tMax;
}

struct Hit
{
    float2 bary;
    float t;
    uint primitiveID;
    bool frontFace;
}

float4
EncodeVisibilityBuffer(float2 bary, uint primitiveID, bool isVisible, bool useSmoothP)
{
    uint encoded = 0;
    if (isVisible)
    {
        encoded |= 0x1 << 0;
    }
    if (useSmoothP)
    {
        encoded |= 0x1 << 1;
    }

    return float4(bary, asfloat(primitiveID), asfloat(encoded));
}

// Surface GetSurface<let T : uint>(RayQuery<T> query, RayDesc ray)
// {
//     return GetSurface(query.CommittedPrimitiveIndex(), query.CommittedTriangleBarycentrics());
// }

void QueryHandleAlpha<let T : uint>(RayQuery<T> query)
{
    if (query.CandidateType() != CANDIDATE_NON_OPAQUE_TRIANGLE)
    {
        return;
    }

    uint primitiveID = query.CandidatePrimitiveIndex();
    float2 bary = query.CandidateTriangleBarycentrics();

    // Surface surf = GetSurface(primitiveID, bary);
    // float mask = Albedo.SampleLevel(samp, uv, 0).r;

    // if (mask >= 0.5)
    // {
    //     query.CommitNonOpaqueTriangleHit();
    // }
}

// hit is not always initialized
#pragma warning(disable : 41018)
bool TraceRay(Ray ray, out Hit hit)
{
    RayDesc rayDesc;
    rayDesc.Origin = ray.P;
    rayDesc.Direction = ray.D;
    rayDesc.TMin = ray.tMin;
    rayDesc.TMax = ray.tMax;

    RayQuery<RAY_FLAG_SKIP_PROCEDURAL_PRIMITIVES> query;
    query.TraceRayInline(TopLevelAS, RAY_FLAG_NONE, 0xFF, rayDesc);

    while (query.Proceed())
    {
#if ALPHA_TEST > 0
        QueryHandleAlpha(query);
#endif
    }

    if (query.CommittedStatus() != COMMITTED_NOTHING)
    {
        hit.bary = query.CommittedTriangleBarycentrics();
        hit.primitiveID = query.CommittedPrimitiveIndex();
        hit.t = query.CommittedRayT();
        hit.frontFace = query.CommittedTriangleFrontFace();
        return true;
    }

    return false;
}
#pragma warning(default : 41018)

bool TraceShadowRay(Ray ray)
{
    RayDesc rayDesc;
    rayDesc.Origin = ray.P;
    rayDesc.Direction = ray.D;
    rayDesc.TMin = ray.tMin;
    rayDesc.TMax = ray.tMax;

    RayQuery<RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH | RAY_FLAG_SKIP_PROCEDURAL_PRIMITIVES> query;
    query.TraceRayInline(TopLevelAS, RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH | RAY_FLAG_SKIP_PROCEDURAL_PRIMITIVES, 0xFF, rayDesc);

    while (query.Proceed())
    {
#if ALPHA_TEST > 0
        QueryHandleAlpha(query);
#endif
    }

    return query.CommittedStatus() != COMMITTED_NOTHING;
}

// "A Fast and Robust Method for Avoiding Self-Intersection" (Carsten Wächter and Nikolaus Binder)
float3 RayOffset(float3 P, float3 Ng)
{
    const float origin = 1.0 / 32.0;
    const float floatScale = 1.0 / 65536.0;
    const float intScale = 256.0;

    // Use int3 for integer vector
    int3 of_i = (int3)float3(intScale * Ng.x, intScale * Ng.y, intScale * Ng.z);

    // Reinterpret bits and offset based on sign
    float3 p_i = float3(
        asfloat(asint(P.x) + ((P.x < 0.0) ? -of_i.x : of_i.x)),
        asfloat(asint(P.y) + ((P.y < 0.0) ? -of_i.y : of_i.y)),
        asfloat(asint(P.z) + ((P.z < 0.0) ? -of_i.z : of_i.z)));

    // Choose between integer-bit offset or small float-scale offset
    return float3(
        abs(P.x) < origin ? P.x + floatScale * Ng.x : p_i.x,
        abs(P.y) < origin ? P.y + floatScale * Ng.y : p_i.y,
        abs(P.z) < origin ? P.z + floatScale * Ng.z : p_i.z);
}

float2 Hammersley(uint i, uint N)
{
    // radical inverse (Van der Corput) in base 2
    uint bits = i;
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    float radicalInverse = float(bits) * 2.3283064365386963e-10; // / 2^32

    return float2(float(i) / float(N), radicalInverse);
}

float3 RandomPointOnSphere(float3 center, float radius, float2 xi)
{
    float z = 1.0 - 2.0 * xi.x;
    float r = sqrt(max(0.0, 1.0 - z * z));
    float phi = 2.0 * float.getPi() * xi.y;

    float3 dir = float3(r * cos(phi), r * sin(phi), z);
    return center + dir * radius;
}

float3 RandomDirectionSphere(float2 xi)
{
    float z = 1.0 - 2.0 * xi.x;
    float phi = 2.0 * float.getPi() * xi.y;

    float r = sqrt(saturate(1.0 - z * z));

    return float3(r * cos(phi), r * sin(phi), z);
}

float3 RandomDirectionInHemisphere(float3 normal, float2 rand)
{
    // rand.x and rand.y are random numbers in [0,1)
    float phi = 2.0 * float.getPi() * rand.x;
    float cosTheta = sqrt(1.0 - rand.y); // cosine-weighted bias
    float sinTheta = sqrt(rand.y);

    // Local direction in tangent space
    float3 localDir = float3(cos(phi) * sinTheta, sin(phi) * sinTheta, cosTheta);

    // Build tangent basis around the normal
    float3 up = abs(normal.z) < 0.999 ? float3(0, 0, 1) : float3(1, 0, 0);
    float3 tangent = normalize(cross(up, normal));
    float3 bitangent = cross(normal, tangent);

    // Transform to world space
    return tangent * localDir.x + bitangent * localDir.y + normal * localDir.z;
}

void AccumulateColor(RWTexture2D<float4> tex, uint2 coords, uint sample, float4 color)
{
    if (sample == 0)
    {
        tex[coords] = color;
    }
    else
    {
        float4 previousColor = LightmapDiffuse[coords];

        float weight = 1.0 / (float(sample) + 1.0);
        float4 accumulatedColor = 1;
        accumulatedColor.rgb = lerp(previousColor.rgb, color.rgb, weight);

        tex[coords] = accumulatedColor;
    }
}

#ifdef LIGHTPROBE_SH
void AccumulateSH(RWStructuredBuffer<SHProbe> buffer, uint probeIndex, Spectrum spectrum)
{
    SHProbe probe = buffer[probeIndex];

    probe.L0 += spectrum.l0;

    probe.L1x += spectrum.l1x;
    probe.L1y += spectrum.l1y;
    probe.L1z += spectrum.l1z;

    buffer[probeIndex] = probe;
}
#endif

struct Spectrum
{

#ifdef LIGHTPROBE_SH
    float3 l0;
    float3 l1x;
    float3 l1y;
    float3 l1z;
#else
    float3 color;
#endif
};

Spectrum SpectrumZero()
{
    Spectrum spectrum;

#ifdef LIGHTPROBE_SH
    spectrum.l0 = 0;
    spectrum.l1x = 0;
    spectrum.l1y = 0;
    spectrum.l1z = 0;
#else
    spectrum.color = 0;
#endif

    return spectrum;
}

static const float L0_NORMALIZATION = 0.5 * sqrt(1.0 / float.getPi());
static const float L1_NORMALIZATION = 0.5 * sqrt(3.0 / float.getPi());
static const float L2_2_NORMALIZATION = 0.5 * sqrt(15.0 / float.getPi());
static const float L20_NORMALIZATION = 0.25 * sqrt(5.0 / float.getPi());
static const float L22_NORMALIZATION = 0.25 * sqrt(15.0 / float.getPi());

static const float A_HAT_0 = float.getPi();
static const float A_HAT_1 = 2.0 * float.getPi() / 3.0;
static const float A_HAT_2 = float.getPi() / 4.0;

void SpectrumAdd(inout Spectrum spectrum, float3 color, float3 wi)
{
#ifdef LIGHTPROBE_SH
    spectrum.l0 += color * L0_NORMALIZATION * A_HAT_0;
    spectrum.l1x += color * L1_NORMALIZATION * wi.x * A_HAT_1;
    spectrum.l1y += color * L1_NORMALIZATION * wi.y * A_HAT_1;
    spectrum.l1z += color * L1_NORMALIZATION * wi.z * A_HAT_1;
#else
    spectrum.color += color;
#endif
}

bool UseSmoothPosition(Surface surface)
{
    float3 rayP = RayOffset(surface.P, surface.Ng);
    float3 diff = surface.smoothP - surface.P;
    float d = length(diff);

    if (d > 0.0001)
    {
        Ray probeRay;
        probeRay.P = rayP;
        probeRay.D = diff / d;
        probeRay.tMin = 0;
        probeRay.tMax = d;

        if (!TraceShadowRay(probeRay))
        {
            return true;
        }
    }

    return false;
}
