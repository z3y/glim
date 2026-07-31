[[vk::image_format("rgba32f")]]
[[vk::binding(0, 0)]]
RWTexture2D<float4> TestTexture;

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    uint2 resolution;
    TestTexture.GetDimensions(resolution.x, resolution.y);

    float2 uv = coords / (float2)resolution;
    uv.y = 1.0 - uv.y;

    TestTexture[coords] = float4(uv, 0, 1);
}
