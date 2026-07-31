#pragma once

#include "bindings.slang"

float2 OctahedronWrap(float2 v)
{
    return (1.0 - abs(v.yx)) * select(v.xy >= 0.0, 1.0, -1.0);
}

float3 DecodeNormalOctahedron(float2 e)
{
    float3 v = float3(e.xy, 1.0 - abs(e.x) - abs(e.y));

    if (v.z < 0.0)
    {
        v.xy = OctahedronWrap(v.xy);
    }

    return normalize(v);
}

float2 EncodeNormalOctahedron(float3 n)
{
    n /= abs(n.x) + abs(n.y) + abs(n.z);

    float2 e = n.xy;

    if (n.z < 0.0)
    {
        e = OctahedronWrap(e);
    }

    return e;
}

float PackNormalOctahedron(float3 normalWS)
{
    float2 oct = EncodeNormalOctahedron(normalWS);

    // [-1,1] -> [0,1]
    oct = oct * 0.5 + 0.5;

    uint x = (uint)round(saturate(oct.x) * 65535.0);
    uint y = (uint)round(saturate(oct.y) * 65535.0);

    return asfloat((x << 16) | y);
}

float3 UnpackNormalOctahedron(float packed)
{
    uint bits = asuint(packed);

    uint x = bits >> 16;
    uint y = bits & 0xFFFF;

    float2 oct;
    oct.x = x / 65535.0;
    oct.y = y / 65535.0;

    // [0,1] -> [-1,1]
    oct = oct * 2.0 - 1.0;

    return DecodeNormalOctahedron(oct);
}

struct Vertex
{
    float3 position;
    uint flags;
    float2 normal;
    float2 uv;

    float3 GetPosition()
    {
        return this.position;
    }

    float3 GetNormal()
    {
        return DecodeNormalOctahedron(this.normal);
    }

    float2 GetUV()
    {
        return this.uv;
    }

    uint GetLightmapGroup()
    {
        return this.flags & 0xFFFFu;
    }

    uint GetFlags()
    {
        return (this.flags >> 16u) & 0xFFFFu;
    }

    bool BackfaceGI()
    {
        return (GetFlags() & (0x1u << 0u)) != 0u;
    }

    bool Emissive()
    {
        return (GetFlags() & (0x1u << 1u)) != 0u;
    }
};

struct Surface
{
    float3 P;
    float3 smoothP;
    float3 Ng;
    float3 N;
    float2 uv; // UVs already scaled by lightmap group resolution

    uint lightmapGroup;
    bool backfaceGI;
    bool emissive;

    static Surface Get(uint primitiveID, float2 bary, bool frontFace = true)
    {
        float u = bary.x;
        float v = bary.y;
        float w = 1.0 - u - v;

        uint i0 = Indices[primitiveID * 3 + 0];
        uint i1 = Indices[primitiveID * 3 + 1];
        uint i2 = Indices[primitiveID * 3 + 2];

        Vertex v0 = Vertices[i0];
        Vertex v1 = Vertices[i1];
        Vertex v2 = Vertices[i2];

        float3 p0 = v0.GetPosition();
        float3 p1 = v1.GetPosition();
        float3 p2 = v2.GetPosition();

        float3 n0 = v0.GetNormal();
        float3 n1 = v1.GetNormal();
        float3 n2 = v2.GetNormal();

        float2 uv0 = v0.GetUV();
        float2 uv1 = v1.GetUV();
        float2 uv2 = v2.GetUV();

        float3 P = p0 * w + p1 * u + p2 * v;
        float3 Ng = normalize(cross(p1 - p0, p2 - p0));
        float3 N = normalize(n0 * w + n1 * u + n2 * v);

        float2 uv = uv0 * w + uv1 * u + uv2 * v;
        // uv.y = 1.0 - uv.y;

        float3 sp0 = P - n0 * dot(P - p0, n0);
        float3 sp1 = P - n1 * dot(P - p1, n1);
        float3 sp2 = P - n2 * dot(P - p2, n2);
        float3 smoothP = sp0 * w + sp1 * u + sp2 * v;

        if (!frontFace)
        {
            N = -N;
            Ng = -Ng;
        }

        Surface surface;
        surface.P = P;
        surface.Ng = Ng;
        surface.N = N;
        surface.smoothP = smoothP;
        surface.uv = uv;
        surface.lightmapGroup = v0.GetLightmapGroup();
        surface.backfaceGI = v0.BackfaceGI();
        surface.emissive = v0.Emissive();
        return surface;
    };
};
