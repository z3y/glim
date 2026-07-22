#pragma once

#include "bindings.slang"
#include "util.slang"

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
EncodeVisibilityBuffer(float2 bary, uint primitiveID, bool isVisible, bool useSmoothP, bool hitSkybox)
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
    if (hitSkybox)
    {
        encoded |= 0x1 << 2;
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
    uint bits = reversebits(i);
    float radicalInverse = float(bits) * 2.3283064365386963e-10;
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

void AccumulateColor(inout float4 previous, uint sampleIndex, float4 color)
{
    if (sampleIndex == 0)
    {
        previous = color;
    }
    else
    {
        float weight = 1.0 / (float(sampleIndex) + 1.0);
        float4 accumulatedColor = lerp(previous, color, weight);
        previous = accumulatedColor;
    }
}

// https://www.ppsloan.org/publications/shdering.pdf
float SHLanczos(float l, float w)
{
    float v = float.getPi() * l / w;
    return pow(sin(v) / v, 4.0);
}
void DeringSHProbeL2(inout SHProbeL2 probe, float w)
{
    float w1 = SHLanczos(1.0, w);
    float w2 = SHLanczos(2.0, w);

    probe.sh[1].xyz *= w1;
    probe.sh[2].xyz *= w1;
    probe.sh[3].xyz *= w1;

    probe.sh[4].xyz *= w2;
    probe.sh[5].xyz *= w2;
    probe.sh[6].xyz *= w2;
    probe.sh[7].xyz *= w2;
    probe.sh[8].xyz *= w2;
}

void AccumulateL2(inout SHProbeL2 previous, uint sampleIndex, float3 color, float3 wi)
{
    // flip Z for Unity
    wi.z = -wi.z;

    previous.sh[0].xyz += color * Y0() * A_HAT_0 * L0_NORMALIZATION;

    previous.sh[1].xyz += color * Y1_1(wi) * A_HAT_1 * L1_NORMALIZATION;
    previous.sh[2].xyz += color * Y10(wi) * A_HAT_1 * L1_NORMALIZATION;
    previous.sh[3].xyz += color * Y11(wi) * A_HAT_1 * L1_NORMALIZATION;

    previous.sh[4].xyz += color * Y2_2(wi) * A_HAT_2 * L2_2_NORMALIZATION;
    previous.sh[5].xyz += color * Y2_1(wi) * A_HAT_2 * L2_2_NORMALIZATION;
    previous.sh[6].xyz += color * Y20(wi) * A_HAT_2 * L20_NORMALIZATION;
    previous.sh[7].xyz += color * Y21(wi) * A_HAT_2 * L2_2_NORMALIZATION;
    previous.sh[8].xyz += color * Y22(wi) * A_HAT_2 * L22_NORMALIZATION;
}

float3 EncodeCombinedSHL1(SHProbeL2 probe)
{
    float3 L0 = probe.sh[0].xyz;
    float3 L1x = probe.sh[3].xyz;
    float3 L1y = probe.sh[1].xyz;
    float3 L1z = probe.sh[2].xyz;

    float3 safeL0 = max(L0, 1e-4);

    const float3 w = float3(0.2126, 0.7152, 0.0722);

    float lumaL0 = max(dot(L0, w), 1e-4);
    float ratioX = dot(L1x, w) / (lumaL0 * 2.0);
    float ratioY = dot(L1y, w) / (lumaL0 * 2.0);
    float ratioZ = dot(L1z, w) / (lumaL0 * 2.0);

    float3 ratio = clamp(float3(ratioX, ratioY, ratioZ), -1.0, 1.0);
    return saturate(mad(ratio, 0.5, 0.5));
}

bool UseSmoothPosition(Surface surface)
{
    float3 rayP = RayOffset(surface.P, surface.Ng);
    float3 diff = surface.smoothP - surface.P;
    float d = length(diff);

    if (d > 0.0001)
    {
        Ray ray;
        ray.P = rayP;
        ray.D = diff / d;
        ray.tMin = 0;
        ray.tMax = d;

        if (!TraceShadowRay(ray))
        {
            return true;
        }
    }

    return false;
}

float2 BaryFromUV(uint primitiveID, float2 uv)
{
    uint i0 = Indices[primitiveID * 3 + 0];
    uint i1 = Indices[primitiveID * 3 + 1];
    uint i2 = Indices[primitiveID * 3 + 2];

    Vertex v0 = Vertices[i0];
    Vertex v1 = Vertices[i1];
    Vertex v2 = Vertices[i2];

    float2 uv0 = v0.GetUV();
    float2 uv1 = v1.GetUV();
    float2 uv2 = v2.GetUV();

    float2 e0 = uv1 - uv0;
    float2 e1 = uv2 - uv0;
    float2 p = uv - uv0;

    float det = e0.x * e1.y - e0.y * e1.x;

    if (abs(det) < 1e-8)
    {
        return float2(0.0, 0.0);
    }

    float invDet = 1.0 / det;

    float b1 = (p.x * e1.y - p.y * e1.x) * invDet;
    float b2 = (-p.x * e0.y + p.y * e0.x) * invDet;
    float b0 = 1.0 - b1 - b2;

    return float2(b1, b2);

    // float2 bary = saturate(float2(b1, b2));
    // float sum = bary.x + bary.y;
    // if (sum > 1.0)
    // {
    //     bary /= sum;
    // }

    // return bary;
}

// uv already multiplied with resolution
uint GetCompactedIndex(float2 uv, uint lightmapGroup, bool flipY = true)
{
    uint2 coords = uint2(floor(uv));
    LightmapInfo info = LightmapGroups[lightmapGroup];

    if (coords.x >= info.resolution.x || coords.y >= info.resolution.y)
    {
        return uint::maxValue;
    }

    if (flipY)
    {
        coords.y = info.resolution.y - 1 - coords.y;
    }

    uint index = InterleaveWithZero(coords.x) | (InterleaveWithZero(coords.y) << 1);

    uint word = index >> 5;
    uint bit = index & 31;

    uint2 entry = CompactionBuffer[info.compactionOffset + word];

    if ((entry.x & (1u << bit)) == 0)
    {
        return uint::maxValue;
    }

    uint rank = countbits(entry.x & ((1u << bit) - 1));
    uint compactIndex = entry.y + rank;
    return compactIndex;
}

float MISWeight(float pdf1, float pdf2)
{
    if (pdf1 <= 0.0)
    {
        return 0.0;
    }
    float ratio = pdf2 / pdf1;
    float denom = 1.0 + ratio * ratio;
    return 1.0 / denom;
}
float Luminance(float3 rgb)
{
    return dot(rgb, float3(0.2126, 0.7152, 0.0722));
}

float3 ShadeTriangleLight(uint primitiveID, float3 positionWS, float3 normalWS, float2 xi, float3 throughput, float randomEmissiveTrianglePdf, out float pdf, out float3 L)
{
    float r0 = sqrt(xi.x);
    float r1 = xi.y;
    float2 bary = float2(r0 * (1.0f - r1), r0 * r1);

    Surface emissiveSurface = Surface::Get(primitiveID, bary);

    float3 lightColor = SampleEmission(emissiveSurface.uv, emissiveSurface.lightmapGroup).rgb / float.getPi();

    float triangleArea = CalculateTriangleArea(primitiveID);

    float pdfArea = randomEmissiveTrianglePdf / triangleArea;

    L = emissiveSurface.P;
    float3 positionToLight = L - positionWS;
    L = normalize(positionToLight);
    float distanceSquare = max(dot(positionToLight, positionToLight), 0.0001);

    float cosTheta = saturate(dot(normalWS, L));

    float cosThetaLight = dot(-L, emissiveSurface.N);
    cosThetaLight = emissiveSurface.backfaceGI ? abs(cosThetaLight) : saturate(cosThetaLight);

    if (cosThetaLight <= 0.001)
    {
        pdf = 0.0;
        return 0.0;
    }

    float solidAngleWeight = cosThetaLight / (distanceSquare * pdfArea);

    float3 color = cosTheta * solidAngleWeight * lightColor * throughput;

    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;
    shadowRay.tMax = max(sqrt(distanceSquare) - 0.001f, 0.0001);
    shadowRay.D = L;
    [branch] if (any(color > 0.0))
    {
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    pdf = pdfArea * distanceSquare / cosThetaLight;

    return color;
}

void ValidateSampleColor(inout float4 color)
{
    if (any(!isfinite(color)))
    {
        color = 0.0;
    }
    color = max(0.0, color);
}

float3 ShadeLightDirect(Light light, float3 positionWS, float3 normalWS, float2 xi, float3 throughput, out float3 L)
{
    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;

    float falloff;
    if (light.type == LightType::Directional)
    {
        L = GetDirectionalLightAngle(light.direction, light.ShadowRangeOrAngle, xi);
        falloff = 1.0;

        shadowRay.tMax = RAY_MAX;
    }
    else
    {
        if (light.type == LightType::Area)
        {
            float2 offset = xi - 0.5;
            float3 right = normalize(cross(light.up, light.direction));
            L = light.position + right * offset.x * light.AreaSize.x + light.up * offset.y * light.AreaSize.y;
        }
        else
        {
            if (light.ShadowRangeOrAngle > 0.0)
            {
                L = RandomPointOnOrientedDisk(light.position, light.ShadowRangeOrAngle, positionWS, xi);
            }
        }

        float3 positionToLight = L - positionWS;
        L = normalize(positionToLight);

        float distanceSquare = dot(positionToLight, positionToLight);

        float rangeSq = light.range * light.range;

        falloff = LightFalloff(distanceSquare, 1.0 / rangeSq);

        if (light.type == LightType::Spot)
        {
            float spotScale;
            float spotOffset;
            GetSpotScaleOffset(light.SpotOuter, light.SpotInnerPercent, spotScale, spotOffset);
            float spotAttenuation = GetSpotAngleAttenuation(light.direction, L, spotScale, spotOffset);

            falloff *= spotAttenuation;
        }

        if (light.type == LightType::Area)
        {
            float area = light.AreaSize.x * light.AreaSize.y;
            float cosLight = saturate(dot(-L, light.direction));
            falloff = (cosLight * area) / (float ::getPi() * distanceSquare);
        }

        shadowRay.tMax = sqrt(distanceSquare);
    }

    float cosTheta = saturate(dot(normalWS, L));

    shadowRay.D = L;

    float3 color = cosTheta * falloff * light.color * throughput;

    [branch] if (any(color > 0.0))
    {
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    return color;
}

float3 ShadeLightDirectSH(Light light, float3 positionWS, float2 xi, float3 throughput, out float3 L)
{
    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;

    float falloff;
    if (light.type == LightType::Directional)
    {
        L = GetDirectionalLightAngle(light.direction, light.ShadowRangeOrAngle, xi);
        falloff = 1.0;

        shadowRay.tMax = RAY_MAX;
    }
    else
    {
        if (light.type == LightType::Area)
        {
            float2 offset = xi - 0.5;
            float3 right = normalize(cross(light.up, light.direction));
            L = light.position + right * offset.x * light.AreaSize.x + light.up * offset.y * light.AreaSize.y;
        }
        else
        {
            if (light.ShadowRangeOrAngle > 0.0)
            {
                L = RandomPointOnOrientedDisk(light.position, light.ShadowRangeOrAngle, positionWS, xi);
            }
        }

        float3 positionToLight = L - positionWS;
        L = normalize(positionToLight);

        float distanceSquare = dot(positionToLight, positionToLight);

        float rangeSq = light.range * light.range;

        falloff = LightFalloff(distanceSquare, 1.0 / rangeSq);

        if (light.type == LightType::Spot)
        {
            float spotScale;
            float spotOffset;
            GetSpotScaleOffset(light.SpotOuter, light.SpotInnerPercent, spotScale, spotOffset);
            float spotAttenuation = GetSpotAngleAttenuation(light.direction, L, spotScale, spotOffset);

            falloff *= spotAttenuation;
        }

        if (light.type == LightType::Area)
        {
            float area = light.AreaSize.x * light.AreaSize.y;
            float cosLight = saturate(dot(-L, light.direction));
            falloff = (cosLight * area) / (float ::getPi() * distanceSquare);
        }

        shadowRay.tMax = sqrt(distanceSquare);
    }

    shadowRay.D = L;

    float3 color = falloff * light.color * throughput;

    [branch] if (any(color > 0.0))
    {
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    return color;
}
