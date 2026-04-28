#pragma once

struct Vertex
{
    float3 position;
    float uvX;
    float3 normal;
    float uvY;

    float3 GetPosition()
    {
        return this.position;
    }

    float3 GetNormal()
    {
        return this.normal;
    }

    float2 GetUV()
    {
        return float2(this.uvX, this.uvY);
    }
};

struct Surface
{
    float3 P;
    float3 smoothP;
    float3 Ng;
    float3 N;
    float2 uv;

    static Surface Get(uint *indices, Vertex *vertices, uint primitiveID, float2 bary)
    {
        float u = bary.x;
        float v = bary.y;
        float w = 1.0 - u - v;

        uint i0 = indices[primitiveID * 3 + 0];
        uint i1 = indices[primitiveID * 3 + 1];
        uint i2 = indices[primitiveID * 3 + 2];

        Vertex v0 = vertices[i0];
        Vertex v1 = vertices[i1];
        Vertex v2 = vertices[i2];

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
        uv.y = 1.0 - uv.y;

        float3 sp0 = P - n0 * dot(P - p0, n0);
        float3 sp1 = P - n1 * dot(P - p1, n1);
        float3 sp2 = P - n2 * dot(P - p2, n2);
        float3 smoothP = sp0 * w + sp1 * u + sp2 * v;

        Surface surface;
        surface.P = P;
        surface.Ng = Ng;
        surface.N = N;
        surface.smoothP = smoothP;
        surface.uv = uv;
        return surface;
    };
};
