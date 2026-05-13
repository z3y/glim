static const float SH_L0_NORM = float(1.0 / 2.0 * sqrt(1.0 / float.getPi()));
static const float SH_L1_NORM = float(1.0 / 2.0 * sqrt(3.0 / float.getPi()));

struct SHProbe
{
    float3 L0;
    float sampleCount;

    float3 L1x;
    float positionX;

    float3 L1y;
    float positionY;

    float3 L1z;
    float positionZ;
};

void AccumulateSH_L1(RWStructuredBuffer<SHProbe> buffer, uint probeIndex, float3 radiance, float3 dir)
{
    buffer[probeIndex].L0.xyz += SH_L0_NORM * radiance;
    buffer[probeIndex].sampleCount += 1.0;

    buffer[probeIndex].L1x.xyz += SH_L1_NORM * radiance * dir.x;
    buffer[probeIndex].L1y.xyz += SH_L1_NORM * radiance * dir.y;
    buffer[probeIndex].L1z.xyz += SH_L1_NORM * radiance * dir.z;
}
