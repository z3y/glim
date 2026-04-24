#include "includes/mesh.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    Vertex *vertices;
    uint *indices;
    uint width;
    uint height;
    float padding0;
    float padding1;
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

[shader("vertex")]
VertexOutput vertex(uint instanceID: SV_VulkanInstanceID)
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
[maxvertexcount(3)]
void geometry(triangle VertexOutput input[3], uint primitiveID: SV_PrimitiveID, inout TriangleStream<Varyings> triStream)
{
    float2 barys[3] = { float2(0, 0), float2(1, 0), float2(0, 1) };

    float2 texelSize = 1.0 / float2(Push.width, Push.height);

    float2 jitter = kJitterOffsets[input[0].instanceID] * (texelSize * 0.5);

    for (int i = 0; i < 3; i++)
    {
        uint vIndex = Push.indices[primitiveID * 3 + i];
        Vertex v = Push.vertices[vIndex];

        float2 uv = v.GetUV() + jitter;

        Varyings outV;
        outV.positionCS = float4(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, 0.0, 1.0);
        outV.bary = barys[i];
        outV.primitiveID = primitiveID;

        triStream.Append(outV);
    }
}

[shader("fragment")]
float4 fragment(Varyings varyings) : SV_Target
{
    // Surface surf = Surface::Get(Push.indices, Push.vertices, varyings.primitiveID, varyings.bary);
    // return float4(surf.N, 1);
    return float4(varyings.bary, asfloat(varyings.primitiveID), 1.0);
}
