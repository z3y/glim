#include "includes/core.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint2 resolution;
    uint groupIndex;
    uint conservative;

    uint2 rtResolution;
    uint2 pad1;
};

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

struct Varyings
{
    float4 positionCS : SV_Position;
    nointerpolation uint primitiveID : TEXCOORD2;
    float2 bary : BARY;
};

static const float2 barys[3] = {float2(0, 0), float2(1, 0), float2(0, 1)};

[shader("vertex")] Varyings vertex(uint vertexID : SV_VulkanVertexID, uint instanceID : SV_VulkanInstanceID)
{
    Varyings o;

    uint primitiveID = vertexID / 3;
    uint index = vertexID % 3;

    uint vIndex = Indices[primitiveID * 3 + index];
    Vertex v = Vertices[vIndex];

    float2 resolution = float2(Push.resolution);
    float2 texelSize = 1.0 / resolution;
    float2 halfTexel = texelSize * 0.5;

    float2 jitter = kJitterOffsets[instanceID] * halfTexel;

    if (Push.conservative > 0)
    {
        jitter = float2(0, 0);
    }

    // bool test = Push.conservative == 1 || instanceID == 24;

    if (v.GetLightmapGroup() != Push.groupIndex)
    {
        o.positionCS = asfloat(-1);
        o.primitiveID = uint::maxValue;
        o.bary = 0.0;
    }
    else
    {
        float2 resolution = Push.resolution;
        float2 rtResolution = Push.rtResolution;

        float2 scale = resolution / rtResolution;
        float2 uv = v.GetUV() / resolution;

        uv += jitter;
        uv.y = 1.0 - uv.y;
        uv *= scale;

        o.positionCS = float4(uv * 2.0 - 1.0, 0.0, 1.0);
        o.primitiveID = primitiveID;
        o.bary = barys[index];
    }

    return o;
}

    [shader("fragment")] float4 fragment(Varyings varyings)
    : SV_Target
{
    float3 bary = float3(varyings.bary, 1.0 - varyings.bary.x - varyings.bary.y);

    bool inside = all(bary >= 0.0);
    if (!inside)
    {
        bary = max(bary, 0);
        float sum = dot(bary, 1);
        if (sum < 1e-6)
        {
            bary /= sum;
        }
    }

    return float4(asfloat(varyings.primitiveID), bary.xy, 1.0);
}
