#include "includes/core.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint width;
    uint height;
    uint groupIndex;
    uint convervative;
};

struct Varyings
{
    float4 positionCS : SV_Position;
    nointerpolation uint primitiveID : TEXCOORD1;
    float2 bary : TEXCOORD2;
};

[shader("vertex")] Varyings vertex(uint vertexID : SV_VulkanVertexID)
{
    Varyings o;

    static const float2 barys[3] = {float2(0, 0), float2(1, 0), float2(0, 1)};

    uint primitiveID = vertexID / 3;
    uint index = vertexID % 3;

    uint vIndex = Indices[primitiveID * 3 + index];
    Vertex v = Vertices[vIndex];

    if (v.GetLightmapGroup() != Push.groupIndex)
    {
        o.positionCS = -1;
        o.primitiveID = 0;
        o.bary = 0;
    }
    else
    {
        float2 resolution = float2(Push.width, Push.height);
        float2 uv = v.GetUV() / resolution;

        // todo write correctly offset into a larger render target
        o.positionCS = float4(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, 0.0, 1.0);
        o.primitiveID = primitiveID;
        o.bary = barys[index];
    }

    return o;
}

    [shader("fragment")] uint2 fragment(Varyings varyings)
    : SV_Target
{
    uint primitiveID = varyings.primitiveID;
    float2 bary = varyings.bary;

    // todo encode bary
    return uint2(primitiveID, 0);

    // return float4(asfloat(primitiveID), bary, 1);
}
