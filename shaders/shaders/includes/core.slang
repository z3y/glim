#pragma once

#include "bindings.slang"

static const float RAY_MAX = 10000.0;

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

void QueryHandleAlpha<let T : uint>(inout RayQuery<T> query)
{
    if (query.CandidateType() != CANDIDATE_NON_OPAQUE_TRIANGLE)
    {
        return;
    }

    uint primitiveID = query.CandidatePrimitiveIndex();
    uint geometryID = query.CandidateGeometryIndex();
    if (geometryID > 0)
    {
        primitiveID += TRANSPARENT_PRIMITIVE_ID_OFFSET;
    }
    float2 bary = query.CandidateTriangleBarycentrics();

    Surface surf = Surface::Get(primitiveID, bary);
    float alpha = SampleAlbedo(surf.uv, surf.lightmapGroup).a;

    if (alpha >= 0.5)
    {
        query.CommitNonOpaqueTriangleHit();
    }
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

    RayQuery query;
    query.TraceRayInline(TopLevelAS, RAY_FLAG_NONE, 0xFF, rayDesc);

    while (query.Proceed())
    {
        QueryHandleAlpha(query);
    }

    if (query.CommittedStatus() == COMMITTED_TRIANGLE_HIT)
    {
        hit.bary = query.CommittedTriangleBarycentrics();
        hit.primitiveID = query.CommittedPrimitiveIndex();
        uint geometryID = query.CommittedGeometryIndex();
        if (geometryID > 0)
        {
            hit.primitiveID += TRANSPARENT_PRIMITIVE_ID_OFFSET;
        }
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

    RayQuery<RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH> query;
    query.TraceRayInline(TopLevelAS, RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH, 0xFF, rayDesc);

    while (query.Proceed())
    {
        QueryHandleAlpha(query);
    }

    return query.CommittedStatus() == COMMITTED_TRIANGLE_HIT;
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

float CalculateTriangleArea(uint primitiveID)
{
    uint i0 = Indices[primitiveID * 3 + 0];
    uint i1 = Indices[primitiveID * 3 + 1];
    uint i2 = Indices[primitiveID * 3 + 2];

    float3 p0 = Vertices[i0].GetPosition();
    float3 p1 = Vertices[i1].GetPosition();
    float3 p2 = Vertices[i2].GetPosition();

    float3 edge1 = p1 - p0;
    float3 edge2 = p2 - p0;

    float3 crossProduct = cross(edge1, edge2);
    float area = 0.5f * length(crossProduct);

    return area;
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

void AccumulateColor(RWTexture2D<float4> tex, uint2 coords, uint sampleIndex, float4 color)
{
    if (sampleIndex == 0)
    {
        tex[coords] = color;
    }
    else
    {
        float4 previousColor = LightmapDiffuse[coords];

        float weight = 1.0 / (float(sampleIndex) + 1.0);
        float4 accumulatedColor = lerp(previousColor, color, weight);

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

    // some ideas from https://ndotl.wordpress.com/2018/08/29/baking-artifact-free-lightmaps/
    float rayLength = (length(worldTexelU) + length(worldTexelV)) * sqrt(2.0);

    float3 u = normalize(worldTexelU - dot(worldTexelU, surface.Ng) * surface.Ng);
    float3 v = normalize(worldTexelV - dot(worldTexelV, surface.Ng) * surface.Ng);
    float3 rayDirs[4] = {u, -u, v, -v};

    Ray leakRay;
    leakRay.P = RayOffset(surface.P, surface.Ng);
    leakRay.tMin = 0;
    leakRay.tMax = rayLength;
    Hit leakHit;
    for (int i = 0; i < 4; i++)
    {
        leakRay.D = rayDirs[i];

        if (TraceRay(leakRay, leakHit))
        {
            if (!leakHit.frontFace)
            {
                // return 0;
            }
        }
    }

    bool smoothP = UseSmoothPosition(surface);
    uint encoded = asuint(visibility.a);
    bool visible = encoded != 0;

    visibility = EncodeVisibilityBuffer(bary, primitiveID, visible, smoothP);

    return visibility;
}
