#include "bindings.slang"
#include "ray.slang"
#include "sobol_burley.slang"
#include "util.slang"

bool TriangleAlphaTest(uint primitiveID, float2 bary)
{
    float u = bary.x;
    float v = bary.y;
    float w = 1.0 - u - v;

    float4 *mainUV = Push.rt.mainUVs;

    float4 uv0 = mainUV[primitiveID * 3 + 0];
    float4 uv1 = mainUV[primitiveID * 3 + 1];
    float4 uv2 = mainUV[primitiveID * 3 + 2];

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

    float4 *bvhNodes = Push.rt.BVHNodes;
    float4 *bvhTriangles = Push.rt.BVHTriangles;

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

            float4 n0 = bvhNodes[childNodeIndex * 5 + 0];
            float4 n1 = bvhNodes[childNodeIndex * 5 + 1];
            float4 n2 = bvhNodes[childNodeIndex * 5 + 2];
            float4 n3 = bvhNodes[childNodeIndex * 5 + 3];
            float4 n4 = bvhNodes[childNodeIndex * 5 + 4];

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
            float4 e1 = bvhTriangles[triAddr + 0];
            float4 e2 = bvhTriangles[triAddr + 1];
            float4 v0 = bvhTriangles[triAddr + 2];

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
        hit.primitiveID = asuint(bvhTriangles[hitTriIndex + 2].w);
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
    uint *indices = Push.rt.indices;
    Vertex *vertices = Push.rt.vertices;
    uint i0 = indices[primitiveID * 3 + 0];
    uint i1 = indices[primitiveID * 3 + 1];
    uint i2 = indices[primitiveID * 3 + 2];

    float3 p0 = vertices[i0].position;
    float3 p1 = vertices[i1].position;
    float3 p2 = vertices[i2].position;

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

float MISWeight(float a, float b)
{
    if (a <= 0.0)
    {
        return 0.0;
    }
    if (isinf(a))
    {
        return 1.0;
    }
    if (isinf(b))
    {
        return 0.0;
    }

    float r = b / a;
    return 1.0 / (1.0 + r * r);
}

float Luminance(float3 rgb)
{
    return dot(rgb, float3(0.2126, 0.7152, 0.0722));
}

// Spherical triangle sampling (Arvo 1995, as in pbrt-v4). Outside this solid angle range it is
// numerically unreliable, so those triangles are area sampled instead.
static const float MIN_SPHERICAL_SAMPLE_AREA = 3e-4;
static const float MAX_SPHERICAL_SAMPLE_AREA = 6.22;

float AngleBetween(float3 a, float3 b)
{
    if (dot(a, b) < 0.0)
    {
        return float.getPi() - 2.0 * asin(saturate(length(a + b) * 0.5));
    }
    return 2.0 * asin(saturate(length(b - a) * 0.5));
}

float SampleLinear(float u, float a, float b)
{
    if (u == 0.0 && a == 0.0)
    {
        return 0.0;
    }
    float x = u * (a + b) / (a + sqrt(lerp(a * a, b * b, u)));
    return min(x, 0.99999994);
}

// w: corner weights (0,0), (1,0), (0,1), (1,1)
float2 SampleBilinear(float2 u, float4 w)
{
    float y = SampleLinear(u.y, w.x + w.y, w.z + w.w);
    float x = SampleLinear(u.x, lerp(w.x, w.z, y), lerp(w.y, w.w, y));
    return float2(x, y);
}

float BilinearPdf(float2 p, float4 w)
{
    if (any(p < 0.0) || any(p > 1.0))
    {
        return 0.0;
    }
    float sum = w.x + w.y + w.z + w.w;
    if (sum == 0.0)
    {
        return 1.0;
    }
    return 4.0 * ((1.0 - p.x) * (1.0 - p.y) * w.x + p.x * (1.0 - p.y) * w.y +
                  (1.0 - p.x) * p.y * w.z + p.x * p.y * w.w) / sum;
}

struct SphericalTriangle
{
    float3 a;
    float3 b;
    float3 c;
    float3 nab;
    float alpha;
    float solidAngle;

    // solidAngle = 0 if degenerate
    static SphericalTriangle Create(float3 p0, float3 p1, float3 p2, float3 p)
    {
        SphericalTriangle t;
        t.a = normalize(p0 - p);
        t.b = normalize(p1 - p);
        t.c = normalize(p2 - p);
        t.nab = 0.0;
        t.alpha = 0.0;
        t.solidAngle = 0.0;

        float3 nab = cross(t.a, t.b);
        float3 nbc = cross(t.b, t.c);
        float3 nca = cross(t.c, t.a);
        if (dot(nab, nab) == 0.0 || dot(nbc, nbc) == 0.0 || dot(nca, nca) == 0.0)
        {
            return t;
        }
        nab = normalize(nab);
        nbc = normalize(nbc);
        nca = normalize(nca);

        t.nab = nab;
        t.alpha = AngleBetween(nab, -nca);
        float beta = AngleBetween(nbc, -nab);
        float gamma = AngleBetween(nca, -nbc);
        t.solidAngle = max(t.alpha + beta + gamma - float.getPi(), 0.0);
        return t;
    }

    // Bilinear warp weights that approximate the receiver cosine over the sample domain.
    float4 CosineWeights(float3 n)
    {
        float wa = max(0.01, abs(dot(n, a)));
        float wb = max(0.01, abs(dot(n, b)));
        float wc = max(0.01, abs(dot(n, c)));
        return float4(wb, wb, wa, wc);
    }

    // Sample() input that produces direction w.
    float2 Invert(float3 w)
    {
        float3 cp = normalize(cross(cross(b, w), cross(c, a)));
        if (dot(cp, a + c) < 0.0)
        {
            cp = -cp;
        }

        float u0 = 0.0;
        if (dot(a, cp) <= 0.99999847691)
        {
            float3 ncpb = cross(cp, b);
            float3 nacp = cross(a, cp);
            if (dot(ncpb, ncpb) == 0.0 || dot(nacp, nacp) == 0.0)
            {
                return 0.5;
            }
            ncpb = normalize(ncpb);
            nacp = normalize(nacp);
            float subArea = alpha + AngleBetween(nab, ncpb) + AngleBetween(nacp, -ncpb) - float.getPi();
            u0 = subArea / solidAngle;
        }

        float u1 = (1.0 - dot(w, b)) / (1.0 - dot(cp, b));
        return saturate(float2(u0, u1));
    }

    bool UseForSampling()
    {
        return solidAngle >= MIN_SPHERICAL_SAMPLE_AREA && solidAngle <= MAX_SPHERICAL_SAMPLE_AREA;
    }

    // Returns the direction, uniform in solid angle.
    float3 Sample(float2 xi)
    {
        float areaPi = lerp(float.getPi(), solidAngle + float.getPi(), xi.x);

        float cosAlpha = cos(alpha);
        float sinAlpha = sin(alpha);
        float sinPhi = sin(areaPi) * cosAlpha - cos(areaPi) * sinAlpha;
        float cosPhi = cos(areaPi) * cosAlpha + sin(areaPi) * sinAlpha;
        float k1 = cosPhi + cosAlpha;
        float k2 = sinPhi - sinAlpha * dot(a, b);
        float cosBp = (k2 + (k2 * cosPhi - k1 * sinPhi) * cosAlpha) / ((k2 * sinPhi + k1 * cosPhi) * sinAlpha);
        cosBp = clamp(cosBp, -1.0, 1.0);
        float sinBp = sqrt(saturate(1.0 - cosBp * cosBp));
        float3 cp = cosBp * a + sinBp * normalize(c - dot(c, a) * a);

        float cosTheta = 1.0 - xi.y * (1.0 - dot(cp, b));
        float sinTheta = sqrt(saturate(1.0 - cosTheta * cosTheta));
        return cosTheta * b + sinTheta * normalize(cp - dot(cp, b) * b);
    }
};

// Barycentrics (Surface::Get convention) of where the ray p + t * w crosses the triangle plane.
float2 TriangleBarycentrics(float3 p0, float3 p1, float3 p2, float3 p, float3 w)
{
    float3 e1 = p1 - p0;
    float3 e2 = p2 - p0;
    float3 s1 = cross(w, e2);
    float divisor = dot(s1, e1);
    if (divisor == 0.0)
    {
        return 0.0;
    }
    float3 s = p - p0;
    float b1 = saturate(dot(s, s1) / divisor);
    float b2 = saturate(dot(w, cross(s, e1)) / divisor);
    float sum = b1 + b2;
    if (sum > 1.0)
    {
        b1 /= sum;
        b2 /= sum;
    }
    return float2(b1, b2);
}

// Solid angle pdf of ShadeTriangleLight picking lightPosition on this triangle, without the
// triangle selection probability.
float TriangleLightPdf(uint primitiveID, float3 positionWS, float3 normalWS, float3 lightPosition, bool isLightProbe)
{
    Vertex v[3] = TriangleVertices(primitiveID);
    float3 p0 = v[0].position;
    float3 p1 = v[1].position;
    float3 p2 = v[2].position;

    float3 toLight = lightPosition - positionWS;
    float distSq = dot(toLight, toLight);
    if (!(distSq > 0.0))
    {
        return 0.0;
    }
    float3 w = toLight * rsqrt(distSq);

    SphericalTriangle st = SphericalTriangle::Create(p0, p1, p2, positionWS);
    if (st.UseForSampling())
    {
        float pdf = 1.0 / st.solidAngle;
        if (!isLightProbe)
        {
            pdf *= BilinearPdf(st.Invert(w), st.CosineWeights(normalWS));
        }
        return pdf;
    }

    float3 n = cross(p1 - p0, p2 - p0);
    float area = 0.5 * length(n);
    float cosLight = abs(dot(w, normalize(n)));
    if (!(area > 0.0) || cosLight < 1e-6)
    {
        return 0.0;
    }
    return distSq / (area * cosLight);
}

float3 ShadeTriangleLight(uint primitiveID, float3 positionWS, float3 normalWS, float2 xi, float3 throughput, float selectionPmf, out float pdf, out float3 L, bool isLightProbe)
{
    pdf = 0.0;
    L = 0.0;

    Vertex v[3] = TriangleVertices(primitiveID);
    float3 p0 = v[0].position;
    float3 p1 = v[1].position;
    float3 p2 = v[2].position;

    SphericalTriangle st = SphericalTriangle::Create(p0, p1, p2, positionWS);
    bool spherical = st.UseForSampling();

    float2 bary;
    float warpPdf = 1.0;
    if (spherical)
    {
        float2 u = xi;
        if (!isLightProbe)
        {
            float4 weights = st.CosineWeights(normalWS);
            u = SampleBilinear(xi, weights);
            warpPdf = BilinearPdf(u, weights);
        }
        bary = TriangleBarycentrics(p0, p1, p2, positionWS, st.Sample(u));
    }
    else
    {
        float r0 = sqrt(xi.x);
        bary = float2(r0 * (1.0 - xi.y), r0 * xi.y);
    }

    Surface emissiveSurface = Surface::Get(primitiveID, bary);

    float3 positionToLight = emissiveSurface.P - positionWS;
    float distanceSquare = dot(positionToLight, positionToLight);
    if (!(distanceSquare > 0.0))
    {
        return 0.0;
    }
    L = positionToLight * rsqrt(distanceSquare);

    float cosLight = dot(-L, emissiveSurface.Ng);
    if (!emissiveSurface.backfaceGI && cosLight <= 0.0)
    {
        return 0.0;
    }

    float directionPdf;
    if (spherical)
    {
        directionPdf = warpPdf / st.solidAngle;
    }
    else
    {
        float area = CalculateTriangleArea(primitiveID);
        if (abs(cosLight) < 1e-6)
        {
            return 0.0;
        }
        directionPdf = distanceSquare / (area * abs(cosLight));
    }

    pdf = selectionPmf * directionPdf;

    float3 lightColor = SampleEmission(emissiveSurface.coords, emissiveSurface.lightmapGroup).rgb / float.getPi();
    float3 color = lightColor * throughput / pdf;

    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;
    shadowRay.tMax = max(sqrt(distanceSquare) - 0.001, 0.0001);
    shadowRay.D = L;

    const float SHADOW_THRESHOLD = 0.001;
    float visible = any(color > SHADOW_THRESHOLD) ? 1 : 0;

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

// Spherical rectangle sampling (Urena et al. 2013). x and y are the unit edge directions.
struct SphericalRectangle
{
    float3 o;
    float3 x;
    float3 y;
    float3 z;
    float x0;
    float x1;
    float y0;
    float y1;
    float z0;
    float b0;
    float b1;
    float k;
    float solidAngle;

    static SphericalRectangle Create(float3 p, float3 corner, float3 x, float3 y, float2 size)
    {
        SphericalRectangle r;
        r.o = p;
        r.x = x;
        r.y = y;
        r.z = cross(x, y);

        float3 d = corner - p;
        r.z0 = dot(d, r.z);
        if (r.z0 > 0.0)
        {
            r.z = -r.z;
            r.z0 = -r.z0;
        }
        r.x0 = dot(d, x);
        r.y0 = dot(d, y);
        r.x1 = r.x0 + size.x;
        r.y1 = r.y0 + size.y;

        float4 diff = float4(r.x0, r.y1, r.x1, r.y0) - float4(r.x1, r.y0, r.x0, r.y1);
        float4 nz = float4(r.y0, r.x1, r.y1, r.x0) * diff;
        nz = nz / sqrt(r.z0 * r.z0 * diff * diff + nz * nz);
        float g0 = acos(clamp(-nz.x * nz.y, -1.0, 1.0));
        float g1 = acos(clamp(-nz.y * nz.z, -1.0, 1.0));
        float g2 = acos(clamp(-nz.z * nz.w, -1.0, 1.0));
        float g3 = acos(clamp(-nz.w * nz.x, -1.0, 1.0));

        r.b0 = nz.x;
        r.b1 = nz.z;
        r.k = 2.0 * float.getPi() - g2 - g3;
        r.solidAngle = g0 + g1 - r.k;
        if (!isfinite(r.solidAngle))
        {
            r.solidAngle = 0.0;
        }
        return r;
    }

    // Small solid angles lose too much precision, area sampling is fine there anyway.
    bool UseForSampling()
    {
        return solidAngle >= 3e-4;
    }

    // Returns the sampled point on the rectangle, uniform in solid angle.
    float3 Sample(float2 xi)
    {
        float au = xi.x * solidAngle + k;
        float fu = (cos(au) * b0 - b1) / sin(au);
        float cu = clamp((fu > 0.0 ? 1.0 : -1.0) * rsqrt(fu * fu + b0 * b0), -1.0, 1.0);
        float xu = clamp(-(cu * z0) / max(sqrt(1.0 - cu * cu), 1e-7), x0, x1);

        float d = sqrt(xu * xu + z0 * z0);
        float h0 = y0 / sqrt(d * d + y0 * y0);
        float h1 = y1 / sqrt(d * d + y1 * y1);
        float hv = lerp(h0, h1, xi.y);
        float hv2 = hv * hv;
        float yv = (hv2 < 1.0 - 1e-6) ? (hv * d) / sqrt(1.0 - hv2) : y1;

        return o + xu * x + yv * y + z0 * z;
    }
};

float3 ShadeLightDirect(Light light, float3 positionWS, float3 normalWS, float2 xi, float3 throughput, out float3 L, bool isLightProbe)
{
    Ray shadowRay;
    shadowRay.P = positionWS;
    shadowRay.tMin = 0;

    float rectSolidAngle = 0.0;
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
            float2 size = light.AreaOrCookieSize;
            float3 corner = light.position - right * (0.5 * size.x) - up * (0.5 * size.y);
            float2 rectUV = xi;

            SphericalRectangle rect = SphericalRectangle::Create(positionWS, corner, right, up, size);
            if (rect.UseForSampling())
            {
                L = rect.Sample(xi);
                rectSolidAngle = rect.solidAngle;
                rectUV = saturate(float2(dot(L - corner, right), dot(L - corner, up)) / max(size, 1e-6));
            }
            else
            {
                L = corner + right * (xi.x * size.x) + up * (xi.y * size.y);
            }

            if (hasCookie)
            {
                float2 cookieUV = float2(1.0 - rectUV.x, rectUV.y);
                cookieColor = SampleCookie(cookieUV, light.cookie).rgb;
            }
        }
        else if (ty == LightType::Disc)
        {
            float radius = light.AreaOrCookieSize.x;
            float2 d;

            // Solid angle sample the bounding square. Points outside the disc contribute
            // nothing, which keeps the estimate unbiased with a bounded weight.
            float3 corner = light.position - (right + up) * radius;
            SphericalRectangle rect = SphericalRectangle::Create(positionWS, corner, right, up, 2.0 * radius);
            if (rect.UseForSampling())
            {
                L = rect.Sample(xi);
                d = float2(dot(L - light.position, right), dot(L - light.position, up)) / max(radius, 1e-6);
                rectSolidAngle = dot(d, d) <= 1.0 ? rect.solidAngle : -1.0;
            }
            else
            {
                d = SampleDiskConcentric(xi);
                L = light.position + (right * d.x + up * d.y) * radius;
            }

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
            float cosLight = saturate(dot(-L, light.direction));
            if (rectSolidAngle > 0.0)
            {
                falloff = cosLight > 0.0 ? rectSolidAngle / float::getPi() : 0.0;
            }
            else
            {
                float area = light.AreaOrCookieSize.x * light.AreaOrCookieSize.y;
                falloff = (cosLight * area) / (float ::getPi() * distanceSquare);
            }
        }
        else if (ty == LightType::Disc)
        {
            float cosLight = saturate(dot(-L, light.direction));
            if (rectSolidAngle != 0.0)
            {
                falloff = (cosLight > 0.0 && rectSolidAngle > 0.0) ? rectSolidAngle / float::getPi() : 0.0;
            }
            else
            {
                float radius = light.AreaOrCookieSize.x;
                float area = float ::getPi() * radius * radius;
                falloff = (cosLight * area) / (float ::getPi() * distanceSquare);
            }
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
        // todo shadow terminator fix
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    return color;
}

void CreateSample(uint index, uint sampleIndex, uint sampleCount, out float3 positionWS, out float3 normalWS, out uint3 indexing)
{
    uint pixelSeed = RNGPixelInit1D(index);
    indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, ~0u, sampleCount, SamplingPattern::BlueNoiseFirst);

    float4 visibility = Push.rt.visibility[index];
    bool moved = asuint(visibility.w) != 0u;

    [branch]
    if (moved || JITTER_BARYCENTRICS == 0)
    {
        positionWS = visibility.xyz;
        normalWS = UnpackNormalOctahedron(visibility.w);
        return;
    }

    float2 bary = visibility.xy;
    uint primitiveID = asuint(visibility.z);

    Vertex[3] v = TriangleVertices(primitiveID);

    float3 p0 = v[0].position;
    float3 p1 = v[1].position;
    float3 p2 = v[2].position;
    float2 uv0 = v[0].uv;
    float2 uv1 = v[1].uv;
    float2 uv2 = v[2].uv;
    uint resolution = v[0].Resolution();

    float2 duv1 = uv1 - uv0;
    float2 duv2 = uv2 - uv0;

    float det = duv1.x * duv2.y - duv1.y * duv2.x;
    float invDetSigned = abs(det) > 1e-12 ? 1.0 / det : 0.0;

    if (JITTER_BARYCENTRICS > 0)
    {
        float2 xiAA = SobolBurleySample2D(indexing.x, DIMENSION_AA, indexing.y, indexing.z);
        // inspired by cycles
        // https://projects.blender.org/blender/cycles/src/branch/main/src/kernel/integrator/init_from_bake.h
        const uint attempts = 10;
        [loop]
        for (uint i = 0; i < attempts; i++)
        {
            // barycentrics here are not always texel centers but centroids
            // jittering always by half a texel means they can end up in another texel
            // but it seems fine for now
            float2 jitterUV = (xiAA - 0.5) / (float)resolution;

            float2 dBary;
            dBary.x = (duv2.y * jitterUV.x - duv2.x * jitterUV.y) * invDetSigned;
            dBary.y = (-duv1.y * jitterUV.x + duv1.x * jitterUV.y) * invDetSigned;

            float2 jBary = bary + dBary;

            bool inside = jBary.x > 0.0 && jBary.y > 0.0 && (jBary.x + jBary.y) < 1.0;
            if (inside)
            {
                bary = jBary;
                break;
            }

            xiAA = hash_float2_to_float2(xiAA);
        }
    }

    bool frontFace = index < BACKFACE_TEXEL_START;
    Surface surface = Surface::Get(primitiveID, bary, frontFace);
    positionWS = RayOffset(surface.P, surface.Ng);
    normalWS = surface.N;
}

// Builds the shadowmask shadow ray, false if the position is outside the light range or spot cone
bool ShadowMaskRay(Light light, float3 positionWS, out Ray shadowRay, out float3 L)
{
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

    return true;
}

bool ShadowMaskVisibility(Light light, float3 positionWS, out float3 L)
{
    Ray shadowRay;
    if (!ShadowMaskRay(light, positionWS, shadowRay, L))
    {
        return false;
    }

    return !TraceShadowRay(shadowRay);
}
