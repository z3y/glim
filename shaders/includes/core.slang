#include "bindings.slang"
#include "ray.slang"
#include "util.slang"

bool TriangleAlphaTest(uint primitiveID, float2 bary)
{
    Surface surf = Surface::Get(primitiveID, bary);
    float alpha = SampleAlbedo(surf.uv, surf.lightmapGroup).a;
    return alpha >= 0.5;
}

uint ExtractByte(uint value, uint byteIndex)
{
    return (value >> (byteIndex * 8)) & 0xFF;
}

float4 ExtractBytes(float value)
{
    uint packed = asuint(value);

    float4 channels = float4(
        ExtractByte(packed, 0),
        ExtractByte(packed, 1),
        ExtractByte(packed, 2),
        ExtractByte(packed, 3));

    return channels;
}

bool BVHTraceRay(Ray ray, out Hit hit, bool anyHit)
{
    hit = {};

    float3 O = ray.P;
    float3 D = ray.D;
    float tmax = ray.tMax;

    float3 invDir = rcp(D);
    uint octinv4 = (7u - ((D.x < 0 ? 4u : 0u) | (D.y < 0 ? 2u : 0u) | (D.z < 0 ? 1u : 0u))) * 0x1010101u;

#pragma warning(push)
#pragma warning(disable: 4700)
    uint2 stack[32];
#pragma warning(pop)

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

            uint slotIndex = (childBitIndex - 24u) ^ (octinv4 & 255u);
            uint relativeIndex = countbits(mask & ~(0xFFFFFFFFu << slotIndex));
            uint childNodeIndex = childNodeBaseIndex + relativeIndex;

            float4 n0 = BVHNodes[childNodeIndex * 5 + 0];
            float4 n1 = BVHNodes[childNodeIndex * 5 + 1];
            float4 n2 = BVHNodes[childNodeIndex * 5 + 2];
            float4 n3 = BVHNodes[childNodeIndex * 5 + 3];
            float4 n4 = BVHNodes[childNodeIndex * 5 + 4];

            uint packed = asuint(n0.w);
            float nodeInvX = asfloat(((ExtractByte(packed, 0) ^ 0x80u) - 0x80u + 127u) << 23) * invDir.x;
            float nodeInvY = asfloat(((ExtractByte(packed, 1) ^ 0x80u) - 0x80u + 127u) << 23) * invDir.y;
            float nodeInvZ = asfloat(((ExtractByte(packed, 2) ^ 0x80u) - 0x80u + 127u) << 23) * invDir.z;
            float3 nodeInvDir = float3(nodeInvX, nodeInvY, nodeInvZ);
            float3 nodePos = (n0.xyz - O) * invDir;

            uint hitmask = 0;

            [unroll]
            for (int i = 0; i < 2; ++i)
            {
                uint meta = asuint(i == 0 ? n1.z : n1.w);

                float4 lox = ExtractBytes(invDir.x < 0 ? (i == 0 ? n3.z : n3.w) : (i == 0 ? n2.x : n2.y));
                float4 loy = ExtractBytes(invDir.y < 0 ? (i == 0 ? n4.x : n4.y) : (i == 0 ? n2.z : n2.w));
                float4 loz = ExtractBytes(invDir.z < 0 ? (i == 0 ? n4.z : n4.w) : (i == 0 ? n3.x : n3.y));
                float4 hix = ExtractBytes(invDir.x < 0 ? (i == 0 ? n2.x : n2.y) : (i == 0 ? n3.z : n3.w));
                float4 hiy = ExtractBytes(invDir.y < 0 ? (i == 0 ? n2.z : n2.w) : (i == 0 ? n4.x : n4.y));
                float4 hiz = ExtractBytes(invDir.z < 0 ? (i == 0 ? n3.x : n3.y) : (i == 0 ? n4.z : n4.w));

                float4 tminx = lox * nodeInvDir.x + nodePos.x;
                float4 tmaxx = hix * nodeInvDir.x + nodePos.x;
                float4 tminy = loy * nodeInvDir.y + nodePos.y;
                float4 tmaxy = hiy * nodeInvDir.y + nodePos.y;
                float4 tminz = loz * nodeInvDir.z + nodePos.z;
                float4 tmaxz = hiz * nodeInvDir.z + nodePos.z;

                float4 cmin = max(max(max(tminx, tminy), tminz), 0.0f);
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
            nodeGroup.y = (hitmask & 0xFF000000u) | (asuint(n0.w) >> 24);
            triGroup.x = asuint(n1.y);
            triGroup.y = hitmask & 0x00FFFFFFu;
        }
        else
        {
            triGroup = nodeGroup;
            nodeGroup = uint2(0, 0u);
        }

        while (triGroup.y != 0u)
        {
            int triangleIndex = firstbithigh(triGroup.y);
            int triAddr = triGroup.x + triangleIndex * 3;
            triGroup.y -= 1u << triangleIndex;

            // CWBVHTriangles layout: [e1, e2, v0] each a float4
            float3 e1 = BVHTriangles[triAddr + 0].xyz;
            float4 e2 = BVHTriangles[triAddr + 1];
            float4 v0 = BVHTriangles[triAddr + 2];

            float3 r = cross(D, e1);
            float a = dot(e2.xyz, r);
            if (abs(a) > 1e-7f)
            {
                float f = 1.0f / a;
                float3 s = O - v0.xyz;
                float u = f * dot(s, r);
                if (u >= 0.0f && u <= 1.0f)
                {
                    float3 q = cross(s, e2.xyz);
                    float v = f * dot(D, q);
                    if (v >= 0.0f && u + v <= 1.0f)
                    {
                        float d = f * dot(e1, q);
                        if (d > 0.0f && d < tmax)
                        {
                            bool transparent = e2.w != 0;
                            uv = float2(u, v);

                            [branch]
                            if (transparent)
                            {
                                bool visible = TriangleAlphaTest(asuint(v0.w), uv);
                                if (!visible)
                                {
                                    continue;
                                }
                            }

                            if (anyHit)
                            {
                                return true;
                            }

                            tmax = d;
                            hitTriIndex = triAddr;
                            hitA = a;
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

        float3 e1 = BVHTriangles[hitTriIndex + 0].xyz;
        float3 e2 = BVHTriangles[hitTriIndex + 1].xyz;
        float4 v0 = BVHTriangles[hitTriIndex + 2];

        hit.primitiveID = asuint(v0.w);
        hit.frontFace = hitA > 0.0;
        hit.t = tmax;

        // hit.p0 = v0.xyz;
        // hit.p1 = v0.xyz + e2;
        // hit.p2 = v0.xyz + e1;
        return true;
    }
    return false;
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
    else
    {
        return BVHTraceRay(ray, hit, false);
    }
}

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
        query.TraceRayInline(TopLevelAS, RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH, 0xFF, rayDesc);

        while (query.Proceed())
        {
            QueryHandleAlpha(query);
        }

        return query.CommittedStatus() == COMMITTED_TRIANGLE_HIT;
    }
    else
    {
        Hit hit = {};
        return BVHTraceRay(ray, hit, true);
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

        if (!TraceShadowRay(ray))
        {
            return true;
        }
    }

    return false;
}

// uv already multiplied with resolution
uint GetCompactedIndex(float2 uv, uint lightmapGroup, bool flipY = true)
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

float3 ShadeTriangleLight(uint primitiveID, float3 positionWS, float3 normalWS, float2 xi, float3 throughput, float randomEmissiveTrianglePdf, out float pdf, out float3 L, bool isLightProbe)
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
    shadowRay.tMax = max(sqrt(distanceSquare) - 0.001f, 0.0001);
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

float3 ShadeLightDirect(Light light, float3 positionWS, float3 normalWS, float2 xi, float3 throughput, out float3 L, bool isLightProbe)
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

        falloff = LightFalloff(distanceSquare, 1.0 / rangeSq, LIGHT_FALLOFF_TYPE);

        if (light.type == LightType::Spot)
        {
            float spotScale;
            float spotOffset;
            SpotLightScaleOffset(light.SpotOuter, light.SpotInnerPercent, spotScale, spotOffset);
            float spotAttenuation = SpotLightAngleAttenuation(light.direction, L, spotScale, spotOffset);

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

    float visible = any(color > 0.0) ? 1 : 0;

    if (!isLightProbe)
    {
        float NoL = dot(normalWS, L);
        visible *= NoL;
    }

    [branch]
    if (visible > 0.0)
    {
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    return color;
}
