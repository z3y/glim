#include "bindings.slang"
#include "ray.slang"
#include "util.slang"

#if 1
bool TriangleAlphaTest(uint primitiveID, float2 bary)
{
    float u = bary.x;
    float v = bary.y;
    float w = 1.0 - u - v;

    float4 uv0 = MainUVs[primitiveID * 3 + 0];
    float4 uv1 = MainUVs[primitiveID * 3 + 1];
    float4 uv2 = MainUVs[primitiveID * 3 + 2];

    float2 uv = uv0.xy * w + uv1.xy * u + uv2.xy * v;

    uint packed = asuint(uv0.z);
    uint textureIndex = packed & 0xFFFF;
    bool wrapUV = (packed & (1u << 16)) != 0;
    float cutoff = uv0.w;

    if (textureIndex != 0xFFFF)
    {
        return SampleAlpha(uv, textureIndex, wrapUV) >= cutoff;
    }

    return true;
}
#else
bool TriangleAlphaTest(uint primitiveID, float2 bary)
{
    Surface surf = Surface::Get(primitiveID, bary);
    float alpha = SampleAlbedo(surf.uv, surf.lightmapGroup).a;
    return alpha >= 0.5;
}
#endif

#pragma warning(disable: 41035)

float4 UnpackBytes(uint p)
{
    return float4((uint4(p, p, p, p) >> uint4(0, 8, 16, 24)) & 0xFFu);
}

float SafeDirComponent(float d)
{
    const float EPS = 1e-15f;
    if (abs(d) < EPS)
        return (asuint(d) & 0x80000000u) != 0u ? -EPS : EPS;
    return d;
}

[ForceInline]
bool BVHTraceRay(Ray ray, out Hit hit, bool shadowRay, bool shadowRenderers)
{
    hit = {};

    float3 O = ray.P;
    float3 D = float3(SafeDirComponent(ray.D.x), SafeDirComponent(ray.D.y), SafeDirComponent(ray.D.z));
    float tmax = ray.tMax;

    float3 invDir = rcp(D);
    bool3 neg = D < 0.0;
    uint octinv = 7u - ((neg.x ? 4u : 0u) | (neg.y ? 2u : 0u) | (neg.z ? 1u : 0u));
    uint octinv4 = octinv * 0x1010101u;

    uint2 stack[32];
    uint stackPtr = 0;
    uint2 nodeGroup = uint2(0, 0x80000000u);
    uint2 triGroup = uint2(0, 0u);

    float2 uv = float2(0, 0);
    uint hitTriIndex = 0;
    float hitA = 0;

    while (true)
    {
        if (nodeGroup.y > 0x00FFFFFFu)
        {
            uint mask = nodeGroup.y;
            uint childBitIndex = firstbithigh(mask);
            uint childNodeBaseIndex = nodeGroup.x;

            nodeGroup.y &= ~(1u << childBitIndex);
            if (nodeGroup.y > 0x00FFFFFFu)
            {
                stack[stackPtr++] = nodeGroup;
            }

            uint slotIndex = (childBitIndex - 24u) ^ octinv;
            uint relativeIndex = countbits(mask & ~(0xFFFFFFFFu << slotIndex));
            uint childNodeIndex = childNodeBaseIndex + relativeIndex;

            float4 n0 = BVHNodes[childNodeIndex * 5 + 0];
            float4 n1 = BVHNodes[childNodeIndex * 5 + 1];
            float4 n2 = BVHNodes[childNodeIndex * 5 + 2];
            float4 n3 = BVHNodes[childNodeIndex * 5 + 3];
            float4 n4 = BVHNodes[childNodeIndex * 5 + 4];

            uint packed = asuint(n0.w);
            int3 ex = (int3(asint(packed)) << int3(24, 16, 8)) >> 24;
            float3 nodeInvDir = asfloat(uint3(ex + 127) << 23) * invDir;
            float3 nodePos = (n0.xyz - O) * invDir;

            uint2 xl = neg.x ? asuint(n3.zw) : asuint(n2.xy);
            uint2 xh = neg.x ? asuint(n2.xy) : asuint(n3.zw);
            uint2 yl = neg.y ? asuint(n4.xy) : asuint(n2.zw);
            uint2 yh = neg.y ? asuint(n2.zw) : asuint(n4.xy);
            uint2 zl = neg.z ? asuint(n4.zw) : asuint(n3.xy);
            uint2 zh = neg.z ? asuint(n3.xy) : asuint(n4.zw);
            uint2 meta2 = asuint(n1.zw);

            uint hitmask = 0;

            [unroll]
            for (int i = 0; i < 2; ++i)
            {
                uint meta = meta2[i];
                float4 lox = UnpackBytes(xl[i]);
                float4 loy = UnpackBytes(yl[i]);
                float4 loz = UnpackBytes(zl[i]);
                float4 hix = UnpackBytes(xh[i]);
                float4 hiy = UnpackBytes(yh[i]);
                float4 hiz = UnpackBytes(zh[i]);

                float4 tminx = lox * nodeInvDir.x + nodePos.x;
                float4 tmaxx = hix * nodeInvDir.x + nodePos.x;
                float4 tminy = loy * nodeInvDir.y + nodePos.y;
                float4 tmaxy = hiy * nodeInvDir.y + nodePos.y;
                float4 tminz = loz * nodeInvDir.z + nodePos.z;
                float4 tmaxz = hiz * nodeInvDir.z + nodePos.z;

                float4 cmin = max(max(max(tminx, tminy), tminz), 0.0);
                float4 cmax = min(min(min(tmaxx, tmaxy), tmaxz), tmax);

                uint isInner = (meta & (meta << 1)) & 0x10101010u;
                uint innerMask = (isInner >> 4) * 0xFFu;
                uint bitIndex = (meta ^ (octinv4 & innerMask)) & 0x1F1F1F1Fu;
                uint childBits = (meta >> 5) & 0x07070707u;

                [unroll]
                for (int j = 0; j < 4; ++j)
                {
                    if (cmin[j] <= cmax[j])
                    {
                        uint shiftBits = (childBits >> (j * 8)) & 255u;
                        uint bitShift = (bitIndex >> (j * 8)) & 31u;
                        hitmask |= shiftBits << bitShift;
                    }
                }
            }

            nodeGroup.x = asuint(n1.x);
            nodeGroup.y = (hitmask & 0xFF000000u) | (packed >> 24);
            triGroup.x = asuint(n1.y);
            triGroup.y = hitmask & 0x00FFFFFFu;
        }

        while (triGroup.y != 0u)
        {
            int triangleIndex = firstbithigh(triGroup.y);
            int triAddr = triGroup.x + triangleIndex * 3;
            triGroup.y -= 1u << triangleIndex;

            // memory layout: [v2-v0, v1-v0, v0]
            float4 e1 = BVHTriangles[triAddr + 0];
            float4 e2 = BVHTriangles[triAddr + 1];
            float4 v0 = BVHTriangles[triAddr + 2];

            if (!shadowRenderers)
            {
                if (shadowRay && e1.w != 0)
                {
                    continue;
                }
            }

            float3 r = cross(D, e1.xyz);
            float a = dot(e2.xyz, r);
            if (a != 0.0)
            {
                float f = 1.0 / a;
                float3 s = O - v0.xyz;
                float u = f * dot(s, r);
                if (u >= 0.0 && u <= 1.0)
                {
                    float3 q = cross(s, e2.xyz);
                    float v = f * dot(D, q);
                    if (v >= 0.0 && u + v <= 1.0)
                    {
                        float d = f * dot(e1.xyz, q);
                        if (d > 0.0 && d < tmax)
                        {

                            [branch]
                            if (e2.w != 0)
                            {
                                if (!TriangleAlphaTest(asuint(v0.w), float2(u, v)))
                                {
                                    continue;
                                }
                            }

                            if (shadowRay)
                            {
                                return true;
                            }

                            tmax = d;
                            hitTriIndex = triAddr;
                            hitA = a;
                            uv = float2(u, v);
                        }
                    }
                }
            }
        }

        if (nodeGroup.y <= 0x00FFFFFFu)
        {
            if (stackPtr > 0)
                nodeGroup = stack[--stackPtr];
            else
                break;
        }
    }

    if (tmax < ray.tMax)
    {
        hit.bary = uv;
        hit.primitiveID = asuint(BVHTriangles[hitTriIndex + 2].w);
        hit.frontFace = hitA > 0.0;
        hit.t = tmax;
        return true;
    }

    return false;
}
#pragma warning(default: 41035)

uint GlobalPrimitiveID(uint group, uint geometryID, uint localPrimitiveID)
{
    uint variant = group * 2 + geometryID;

    [flatten]
    if (variant == 1)
    {
        return localPrimitiveID + GEOMETRIES_1_OFFSET;
    }
    else if (variant == 2)
    {
        return localPrimitiveID + GEOMETRIES_2_OFFSET;
    }
    else if (variant == 3)
    {
        return localPrimitiveID + GEOMETRIES_3_OFFSET;
    }
    else
    {
        return localPrimitiveID; // no offset
    }
}

void QueryHandleAlpha<let T : uint>(inout RayQuery<T> query)
{
    if (query.CandidateType() != CANDIDATE_NON_OPAQUE_TRIANGLE)
    {
        return;
    }

    uint primitiveID = query.CandidatePrimitiveIndex();
    uint geometryID = query.CandidateGeometryIndex();
    uint group = query.CandidateInstanceID();

    primitiveID = GlobalPrimitiveID(group, geometryID, primitiveID);

    float2 bary = query.CandidateTriangleBarycentrics();

    bool visible = TriangleAlphaTest(primitiveID, bary);

    if (visible)
    {
        query.CommitNonOpaqueTriangleHit();
    }
}

bool TraceRay(Ray ray, out Hit hit)
{
    hit = {};

    if (HARDWARE_RT == 1)
    {
        RayDesc rayDesc;
        rayDesc.Origin = ray.P;
        rayDesc.Direction = ray.D;
        rayDesc.TMin = ray.tMin;
        rayDesc.TMax = ray.tMax;

        RayQuery query;
        query.TraceRayInline(TopLevelAS, RAY_FLAG_NONE, 0x01, rayDesc);

        while (query.Proceed())
        {
            QueryHandleAlpha(query);
        }

        if (query.CommittedStatus() == COMMITTED_TRIANGLE_HIT)
        {
            hit.bary = query.CommittedTriangleBarycentrics();
            hit.primitiveID = query.CommittedPrimitiveIndex();
            uint geometryID = query.CommittedGeometryIndex();
            uint group = query.CommittedInstanceID();

            hit.primitiveID = GlobalPrimitiveID(group, geometryID, hit.primitiveID);

            hit.t = query.CommittedRayT();
            hit.frontFace = query.CommittedTriangleFrontFace();
            return true;
        }
        return false;
    }
    else
    {
        return BVHTraceRay(ray, hit, false, false);
    }
}

// exclude renderers with no shadow casting
bool TraceShadowRay(Ray ray)
{
    if (HARDWARE_RT == 1)
    {
        RayDesc rayDesc;
        rayDesc.Origin = ray.P;
        rayDesc.Direction = ray.D;
        rayDesc.TMin = ray.tMin;
        rayDesc.TMax = ray.tMax;

        RayQuery<RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH> query;
        query.TraceRayInline(TopLevelAS, RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH, 0x02, rayDesc);

        while (query.Proceed())
        {
            QueryHandleAlpha(query);
        }

        return query.CommittedStatus() == COMMITTED_TRIANGLE_HIT;
    }
    else
    {
        Hit hit = {};
        return BVHTraceRay(ray, hit, true, false);
    }
}

// includes renderers with no shadow casting
bool TraceAnyHit(Ray ray)
{
    if (HARDWARE_RT == 1)
    {
        RayDesc rayDesc;
        rayDesc.Origin = ray.P;
        rayDesc.Direction = ray.D;
        rayDesc.TMin = ray.tMin;
        rayDesc.TMax = ray.tMax;

        RayQuery<RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH> query;
        query.TraceRayInline(TopLevelAS, RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH, 0x01, rayDesc);

        while (query.Proceed())
        {
            QueryHandleAlpha(query);
        }

        return query.CommittedStatus() == COMMITTED_TRIANGLE_HIT;
    }
    else
    {
        Hit hit = {};
        return BVHTraceRay(ray, hit, true, true);
    }
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

float3 RandomDirectionInSphere(float2 xi)
{
    float z = 1.0 - 2.0 * xi.x;
    float phi = 2.0 * float.getPi() * xi.y;

    float r = sqrt(saturate(1.0 - z * z));

    return float3(r * cos(phi), r * sin(phi), z);
}

float3 RandomDirectionInHemisphereCosine(float3 normal, float2 rand)
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

void AccumulateL1LightmapDirect(inout SHProbeL2 previous, float3 color, float3 wi, float3 normalWS)
{
    // flip Z for Unity
    wi.z = -wi.z;
    normalWS.z = -normalWS.z;

    float NoL = saturate(dot(wi, normalWS) * 4.0);
    float AY0 = 0.25;
    float AY1 = 0.50;

    color *= NoL;

    if (NoL > 0.001)
    {
        previous.sh[0].xyz += color * AY0;

        float luma = Luminance(color);

        previous.sh[3].xyz += luma * wi.x * AY1;
        previous.sh[1].xyz += luma * wi.y * AY1;
        previous.sh[2].xyz += luma * wi.z * AY1;
    }
}

void AccumulateL1LightmapIndirect(inout SHProbeL2 previous, float3 color, float3 wi, float3 normalWS)
{
    float NoL = saturate(dot(wi, normalWS));
    color *= 1.0 / NoL;

    // flip Z for Unity
    wi.z = -wi.z;
    normalWS.z = -normalWS.z;

    color *= saturate(NoL * 4.0);

    float AY0 = 0.25;
    float AY1 = 0.50;
    if (NoL > 0.001)
    {
        previous.sh[0].xyz += color * AY0;

        float luma = Luminance(color);

        previous.sh[3].xyz += luma * wi.x * AY1;
        previous.sh[1].xyz += luma * wi.y * AY1;
        previous.sh[2].xyz += luma * wi.z * AY1;
    }
}

float3 EncodeCombinedSHL1(SHProbeL2 probe)
{
    float3 L1x = probe.sh[3].xyz;
    float3 L1y = probe.sh[1].xyz;
    float3 L1z = probe.sh[2].xyz;

    float3 combined = float3(L1x.x, L1y.x, L1z.x);
    return combined;
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

        if (!TraceAnyHit(ray))
        {
            return true;
        }
    }

    return false;
}

// uv already multiplied with resolution
uint GetCompactedIndex(float2 uv, uint lightmapGroup, uint primitiveID, bool flipY = true)
{
    uint2 coords = uint2(floor(uv));
    LightmapInfo info = LightmapsInfo[lightmapGroup];

    if (coords.x >= info.resolution.x || coords.y >= info.resolution.y)
    {
        return uint::maxValue;
    }

    if (flipY)
    {
        coords.y = info.resolution.y - 1 - coords.y;
    }

    uint index = coords.y * info.resolution.x + coords.x;

    uint word = index >> 5;
    uint bit = index & 31;

    uint2 entry = CompactionBuffer[info.compactionOffset + word];

    if ((entry.x & (1u << bit)) == 0)
    {
        return uint::maxValue;
    }

    uint rank = countbits(entry.x & ((1u << bit) - 1));
    uint cellRank = entry.y + rank;

    uint2 run = CompactionBuffer[info.cellTableBase + cellRank];

    // Prefer the texel that belongs to the triangle we actually hit.
    uint end = run.x + run.y;
    for (uint i = run.x; i < end; i++)
    {
        if (asuint(CompactedVisibility[i].z) == primitiveID)
        {
            return i;
        }
    }
    if (asuint(CompactedVisibility[run.x].z) != uint::maxValue)
    {
        // first in the run is the best-covered texel
        return run.x;
    }
    return uint::maxValue;
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

float3 ShadeTriangleLight(uint primitiveID, float3 positionWS, float3 normalWS, float2 xi, float3 throughput, float randomEmissiveTrianglePdf, out float pdf, out float3 L, bool isLightProbe)
{
    float r0 = sqrt(xi.x);
    float r1 = xi.y;
    float2 bary = float2(r0 * (1.0 - r1), r0 * r1);

    Surface emissiveSurface = Surface::Get(primitiveID, bary);

    float3 lightColor = SampleEmission(emissiveSurface.uv, emissiveSurface.lightmapGroup).rgb / float.getPi();

    float triangleArea = CalculateTriangleArea(primitiveID);

    float pdfArea = randomEmissiveTrianglePdf / triangleArea;

    L = emissiveSurface.P;
    float3 positionToLight = L - positionWS;
    L = normalize(positionToLight);
    float distanceSquare = max(dot(positionToLight, positionToLight), 0.0001);

    float cosThetaLight = dot(-L, emissiveSurface.N);
    cosThetaLight = emissiveSurface.backfaceGI ? abs(cosThetaLight) : saturate(cosThetaLight);

    if (cosThetaLight <= 0.001)
    {
        pdf = 0.0;
        return 0.0;
    }

    float solidAngleWeight = cosThetaLight / (distanceSquare * pdfArea);

    float3 color = solidAngleWeight * lightColor * throughput;

    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;
    shadowRay.tMax = max(sqrt(distanceSquare) - 0.001, 0.0001);
    shadowRay.D = L;

    float visible = any(color > 0.0) ? 1 : 0;

    if (!isLightProbe)
    {
        float NoL = dot(normalWS, L);
        visible *= NoL;
    }

    [branch]
    if (visible > 0.0)
    {
        if (TraceAnyHit(shadowRay))
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

float3 ShadeLightDirect(Light light, float3 positionWS, float3 normalWS, float2 xi, float3 throughput, out float3 L, bool isLightProbe)
{
    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;

    float3 cookieColor = 1.0;
    bool hasCookie = light.cookie != uint::maxValue;

    float3 fwd = light.direction;
    float3 right = normalize(cross(light.up, fwd));
    float3 up = light.up;

    LightType ty = light.Type();

    float falloff;
    if (ty == LightType::Directional)
    {
        L = GetDirectionalLightAngle(light.direction, light.ShadowRadiusOrAngle, xi);
        falloff = 1.0;

        shadowRay.tMax = RAY_MAX;

        if (hasCookie)
        {
            float3 toPoint = positionWS - light.position;
            float2 xy = float2(dot(toPoint, right), dot(toPoint, up));

            float2 cookieUV = xy / max(light.AreaOrCookieSize, 1e-4) + 0.5;
            cookieUV = frac(cookieUV);

            cookieColor = SampleCookie(cookieUV, light.cookie).rgb;
        }
    }
    else
    {
        if (ty == LightType::Area)
        {
            float2 offset = xi - 0.5;
            L = light.position + right * offset.x * light.AreaOrCookieSize.x + light.up * offset.y * light.AreaOrCookieSize.y;

            if (hasCookie)
            {
                float2 cookieUV = float2(1.0 - xi.x, xi.y);
                cookieColor = SampleCookie(cookieUV, light.cookie).rgb;
            }
        }
        else if (ty == LightType::Disc)
        {
            float radius = light.AreaOrCookieSize.x;
            float2 d = SampleDiskConcentric(xi);
            L = light.position + (right * d.x + up * d.y) * radius;

            if (hasCookie)
            {
                float2 cookieUV = float2(1.0 - (d.x * 0.5 + 0.5), d.y * 0.5 + 0.5);
                cookieColor = SampleCookie(cookieUV, light.cookie).rgb;
            }
        }
        else
        {
            L = light.position;
            if (light.ShadowRadiusOrAngle > 0.0)
            {
                L = RandomPointOnOrientedDisk(light.position, light.ShadowRadiusOrAngle, positionWS, xi);
            }
        }

        float3 positionToLight = L - positionWS;
        L = normalize(positionToLight);

        float distanceSquare = dot(positionToLight, positionToLight);

        float rangeSq = light.range * light.range;

        falloff = LightFalloff(distanceSquare, 1.0 / rangeSq, LIGHT_FALLOFF_TYPE);

        if (ty == LightType::Spot)
        {
            float spotScale;
            float spotOffset;
            SpotLightScaleOffset(light.SpotOuter, light.SpotInnerPercent, spotScale, spotOffset);
            float3 toLightCenter = normalize(light.position - positionWS);
            float spotAttenuation = SpotLightAngleAttenuation(light.direction, toLightCenter, spotScale, spotOffset);

            falloff *= spotAttenuation;

            if (hasCookie)
            {
                float3 up = cross(right, fwd);

                float3 toPoint = positionWS - light.position;

                float z = dot(toPoint, fwd);
                float2 xy = float2(dot(toPoint, right), dot(toPoint, up));

                float tanHalfAngle = tan(radians(light.SpotOuter) * 0.5);

                float2 cookieUV = xy / (max(z, 1e-4) * tanHalfAngle);
                cookieUV = cookieUV * 0.5 + 0.5;
                cookieUV = 1.0 - cookieUV;

                // cookieColor = float3(cookieUV, 0.0);
                cookieColor = SampleCookie(cookieUV, light.cookie).rgb;
            }
        }

        if (ty == LightType::Point && hasCookie)
        {
            float3 d = positionWS - light.position;
            float3 dirLocal = float3(dot(d, right), dot(d, up), dot(d, fwd));
            cookieColor = SampleCookieOctahedral(dirLocal, light.cookie).rgb;
        }

        if (ty == LightType::Area)
        {
            float area = light.AreaOrCookieSize.x * light.AreaOrCookieSize.y;
            float cosLight = saturate(dot(-L, light.direction));
            falloff = (cosLight * area) / (float ::getPi() * distanceSquare);
        }
        else if (ty == LightType::Disc)
        {
            float radius = light.AreaOrCookieSize.x;
            float area = float ::getPi() * radius * radius;
            float cosLight = saturate(dot(-L, light.direction));
            falloff = (cosLight * area) / (float ::getPi() * distanceSquare);
        }

        shadowRay.tMax = sqrt(distanceSquare);
    }

    shadowRay.D = L;

    float3 color = falloff * light.color * cookieColor * throughput;

    float visible = any(color > 0.0) ? 1 : 0;

    if (!isLightProbe)
    {
        float NoL = dot(normalWS, L);
        visible *= NoL;
    }

    [branch]
    if (visible > 0.0 && light.CastShadows())
    {
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    return color;
}

bool ShadowMaskVisibility(Light light, float3 positionWS, out float3 L)
{
    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;

    LightType ty = light.Type();

    if (ty == LightType::Directional)
    {
        shadowRay.D = light.direction;
        shadowRay.tMax = RAY_MAX;

        L = light.position;
    }
    else
    {
        float3 positionToLight = light.position - positionWS;
        float distanceSquare = dot(positionToLight, positionToLight);

        L = normalize(positionToLight);

        if (distanceSquare > (light.range * light.range))
        {
            return false;
        }

        if (ty == LightType::Spot)
        {
            float spotScale;
            float spotOffset;
            SpotLightScaleOffset(light.SpotOuter + 1, light.SpotInnerPercent, spotScale, spotOffset);

            if (SpotLightAngleAttenuation(light.direction, L, spotScale, spotOffset) <= 0.0)
            {
                return false;
            }
        }

        shadowRay.D = L;
        shadowRay.tMax = sqrt(distanceSquare);
    }

    return !TraceShadowRay(shadowRay);
}

Vertex[3] TriangleVertices(uint primitiveID)
{
    uint i0 = Indices[primitiveID * 3 + 0];
    uint i1 = Indices[primitiveID * 3 + 1];
    uint i2 = Indices[primitiveID * 3 + 2];

    return { Vertices[i0], Vertices[i1], Vertices[i2] };
}

bool CreateSample(uint index, float2 xi, out float3 positionWS, out float3 normalWS)
{
    float4 visibility = CompactedVisibility[index];
    float2 bary = visibility.xy;
    uint primitiveID = asuint(visibility.z);

    if (primitiveID == uint::maxValue)
    {
        positionWS = 0.0;
        normalWS = 0.0;
        return false;
    }
    // uint x = asuint(visibility.w) & 0xFFFF;
    // uint y = (asuint(visibility.w) >> 16) & 0xFFFF;

    Vertex[3] v = TriangleVertices(primitiveID);

    float3 p0 = v[0].GetPosition();
    float3 p1 = v[1].GetPosition();
    float3 p2 = v[2].GetPosition();
    float2 uv0 = v[0].GetUV();
    float2 uv1 = v[1].GetUV();
    float2 uv2 = v[2].GetUV();

    float3 edge1 = p1 - p0;
    float3 edge2 = p2 - p0;
    float2 duv1 = uv1 - uv0;
    float2 duv2 = uv2 - uv0;

    float det = duv1.x * duv2.y - duv1.y * duv2.x;
    float invDet = 1.0 / (abs(det) + 1e-9);
    float invDetSigned = abs(det) > 1e-12 ? 1.0 / det : 0.0;

    float3 dPdU = (edge1 * duv2.y - edge2 * duv1.y) * invDet;
    float3 dPdV = (edge2 * duv1.x - edge1 * duv2.x) * invDet;

    float3 worldTexelU = dPdU;
    float3 worldTexelV = dPdV;

    float2 jitterTexel = xi - 0.5;

    float2 dBary;
    dBary.x = (duv2.y * jitterTexel.x - duv2.x * jitterTexel.y) * invDetSigned;
    dBary.y = (-duv1.y * jitterTexel.x + duv1.x * jitterTexel.y) * invDetSigned;

    float3 w0 = float3(1.0 - bary.x - bary.y, bary.x, bary.y);
    float3 dw = float3(-dBary.x - dBary.y, dBary.x, dBary.y);

    // Clip the jitter segment [bary, bary + dBary] to the triangle instead of
    // projecting the end point. Barycentrics: w = (1 - x - y, x, y)
    float t = 1.0;
    [unroll]
    for (int k = 0; k < 3; k++)
    {
        if (dw[k] < 0.0)
        {
            // largest t where w0[k] + t * dw[k] >= 0
            t = min(t, max(w0[k], 0.0) / -dw[k]);
        }
    }

    float2 jBary = bary + dBary * t;

    // float-error safety only
    jBary = max(jBary, 0.0);
    float s = jBary.x + jBary.y;
    if (s > 1.0)
    {
        jBary /= s;
    }

    Surface surface = Surface::Get(primitiveID, jBary);

    positionWS = RayOffset(surface.P, surface.Ng);
    normalWS = surface.N;

    const bool leakDetection = true;
    if (leakDetection)
    {
        float rayLength = (length(worldTexelU) + length(worldTexelV)) * sqrt(0.5);
        float3 u = normalize(worldTexelU - dot(worldTexelU, surface.Ng) * surface.Ng);
        float3 v = normalize(worldTexelV - dot(worldTexelV, surface.Ng) * surface.Ng);
        float3 rayDirs[4] = { u, -u, v, -v };

        Ray ray;
        ray.P = positionWS;
        ray.tMin = 0;
        ray.tMax = rayLength;
        Hit leakHit;

        [loop]
        for (int i = 0; i < 4; i++)
        {
            ray.D = rayDirs[i];

            if (TraceRay(ray, leakHit))
            {
                if (!leakHit.frontFace)
                {
                    Surface hitSurf = Surface::Get(leakHit.primitiveID, leakHit.bary, true);
                    if (!hitSurf.backfaceGI)
                    {
                        positionWS = RayOffset(hitSurf.P, hitSurf.Ng);
                        break;
                    }
                }
            }
        }
    }

    return true;
}
