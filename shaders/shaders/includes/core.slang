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

    probe.sh[0].xyz += spectrum.sh[0];

    probe.sh[1].xyz += spectrum.sh[1];
    probe.sh[2].xyz += spectrum.sh[2];
    probe.sh[3].xyz += spectrum.sh[3];

    probe.sh[4].xyz += spectrum.sh[4];
    probe.sh[5].xyz += spectrum.sh[5];
    probe.sh[6].xyz += spectrum.sh[6];
    probe.sh[7].xyz += spectrum.sh[7];
    probe.sh[8].xyz += spectrum.sh[8];

    buffer[probeIndex] = probe;
}
#endif

struct Spectrum
{
#ifdef LIGHTPROBE_SH
    float3 sh[9];
#else
    float3 color;
#endif
};

Spectrum SpectrumZero()
{
    Spectrum spectrum;

#ifdef LIGHTPROBE_SH
    spectrum.sh = {0};
#else
    spectrum.color = 0;
#endif

    return spectrum;
}

void SpectrumAdd(inout Spectrum spectrum, float3 color, float3 wi)
{
#ifdef LIGHTPROBE_SH
    // flip Z for Unity
    wi.z = -wi.z;

    spectrum.sh[0] += color * Y0() * A_HAT_0 * L0_NORMALIZATION;

    spectrum.sh[1] += color * Y1_1(wi) * A_HAT_1 * L1_NORMALIZATION;
    spectrum.sh[2] += color * Y10(wi) * A_HAT_1 * L1_NORMALIZATION;
    spectrum.sh[3] += color * Y11(wi) * A_HAT_1 * L1_NORMALIZATION;

    spectrum.sh[4] += color * Y2_2(wi) * A_HAT_2 * L2_2_NORMALIZATION;
    spectrum.sh[5] += color * Y2_1(wi) * A_HAT_2 * L2_2_NORMALIZATION;
    spectrum.sh[6] += color * Y20(wi) * A_HAT_2 * L20_NORMALIZATION;
    spectrum.sh[7] += color * Y21(wi) * A_HAT_2 * L2_2_NORMALIZATION;
    spectrum.sh[8] += color * Y22(wi) * A_HAT_2 * L22_NORMALIZATION;
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

float4 AdjustSamplePositions(Surface surface, float4 visibility)
{
    float3 originalP = surface.P;

    uint primitiveID = asuint(visibility.z);
    float2 bary = visibility.xy;

    uint2 resolution;
    VisibilityBuffer.GetDimensions(resolution.x, resolution.y);

    uint i0 = Indices[primitiveID * 3 + 0];
    uint i1 = Indices[primitiveID * 3 + 1];
    uint i2 = Indices[primitiveID * 3 + 2];

    float3 p0 = Vertices[i0].GetPosition();
    float3 p1 = Vertices[i1].GetPosition();
    float3 p2 = Vertices[i2].GetPosition();
    float2 uv0 = Vertices[i0].GetUV();
    float2 uv1 = Vertices[i1].GetUV();
    float2 uv2 = Vertices[i2].GetUV();

    float3 edge1 = p1 - p0;
    float3 edge2 = p2 - p0;
    float2 duv1 = uv1 - uv0;
    float2 duv2 = uv2 - uv0;

    float det = duv1.x * duv2.y - duv1.y * duv2.x;
    float invDet = 1.0 / (abs(det) + 1e-9);

    float3 dPdU = (edge1 * duv2.y - edge2 * duv1.y) * invDet;
    float3 dPdV = (edge2 * duv1.x - edge1 * duv2.x) * invDet;

    float2 texelSizeUV = 1.0 / float2(resolution);
    float3 worldTexelU = dPdU * texelSizeUV.x;
    float3 worldTexelV = dPdV * texelSizeUV.y;

    float rayLength = (length(worldTexelU) + length(worldTexelV)) * 0.5;

    float3 u = normalize(worldTexelU - dot(worldTexelU, surface.Ng) * surface.Ng);
    float3 v = normalize(worldTexelV - dot(worldTexelV, surface.Ng) * surface.Ng);
    float3 rayDirs[4] = {u, -u, v, -v};

    float3 correctedP = surface.P;
    float3 correctedNg = surface.Ng;
    bool foundLeak = false;
    float bias = 0.00001;

    // https://ndotl.wordpress.com/2018/08/29/baking-artifact-free-lightmaps/
    for (int i = 0; i < 4; i++)
    {
        Ray leakRay;
        leakRay.P = RayOffset(surface.P, surface.Ng);
        leakRay.D = rayDirs[i];
        leakRay.tMin = 0;
        leakRay.tMax = rayLength;

        Hit leakHit;
        if (TraceRay(leakRay, leakHit))
        {
            Surface hitSurface = Surface::Get(leakHit.primitiveID, leakHit.bary);

            bool isBackface = dot(hitSurface.Ng, leakRay.D) > 0;

            if (isBackface)
            {
                correctedP = surface.P + (rayDirs[i] * leakHit.t) + (hitSurface.Ng * bias);
                foundLeak = true;
                break;
            }
        }
    }

    if (foundLeak)
    {
        Ray findNewTri;
        findNewTri.P = RayOffset(correctedP, surface.Ng);
        findNewTri.D = -surface.Ng;
        findNewTri.tMin = 0;
        findNewTri.tMax = 0.001;

        Hit newHit;
        if (TraceRay(findNewTri, newHit))
        {
            primitiveID = newHit.primitiveID;
            bary = newHit.bary;
        }
    }

    visibility.xy = bary;
    visibility.z = asfloat(primitiveID);

    bool smoothP = UseSmoothPosition(surface);
    uint encoded = asuint(visibility.a);
    bool visible = encoded != 0;

    visibility = EncodeVisibilityBuffer(bary, primitiveID, visible, smoothP);

    return visibility;
}
