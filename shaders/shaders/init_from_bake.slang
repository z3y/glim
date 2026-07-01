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
    float2 bary : TEXCOORD0;
    nointerpolation uint primitiveID : TEXCOORD1;
};

struct VertexOutput
{
    uint instanceID : TEXCOORD0;
};

[shader("vertex")] VertexOutput vertex(uint instanceID : SV_VulkanInstanceID)
{
    VertexOutput o;
    o.instanceID = instanceID;
    return o;
}

// https://ndotl.wordpress.com/2018/08/29/baking-artifact-free-lightmaps/
static const float2 kJitterOffsets[25] = {
    float2(-2, -2),
    float2(2, -2),
    float2(-2, 2),
    float2(2, 2),

    float2(-1, -2),
    float2(1, -2),
    float2(-2, -1),
    float2(2, -1),
    float2(-2, 1),
    float2(2, 1),
    float2(-1, 2),
    float2(1, 2),

    float2(-2, 0),
    float2(2, 0),
    float2(0, -2),
    float2(0, 2),

    float2(-1, -1),
    float2(1, -1),
    float2(-1, 0),
    float2(1, 0),
    float2(-1, 1),
    float2(1, 1),
    float2(0, -1),
    float2(0, 1),

    float2(0, 0),
};

[shader("geometry")]
    [maxvertexcount(3)] void
    geometry(triangle VertexOutput input[3], uint primitiveID : SV_PrimitiveID, inout TriangleStream<Varyings> triStream)
{
    float2 barys[3] = {float2(0, 0), float2(1, 0), float2(0, 1)};

    float2 texelSize = 1.0 / float2(Push.width, Push.height);

    float2 jitter = kJitterOffsets[input[0].instanceID] * (texelSize * 0.5);

    if (Push.convervative > 0)
    {
        jitter = float2(0, 0);
    }

    for (int i = 0; i < 3; i++)
    {
        uint vIndex = Indices[primitiveID * 3 + i];
        Vertex v = Vertices[vIndex];

        if (v.GetLightmapGroup() != Push.groupIndex)
        {
            continue;
        }

        float2 uv = v.GetUV() + jitter;

        Varyings outV;
        outV.positionCS = float4(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, 0.0, 1.0);
        outV.bary = barys[i];
        outV.primitiveID = primitiveID;

        triStream.Append(outV);
    }
}

    [shader("fragment")] float4 fragment(Varyings varyings)
    : SV_Target
{
    float2 bary = saturate(varyings.bary);
    // todo sample alpha here
    return EncodeVisibilityBuffer(bary, varyings.primitiveID, true, false);
}
