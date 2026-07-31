#pragma once

struct SHProbeL2
{
    float3 position;
    float radius;
    float4 sh[9];

    static SHProbeL2 zero()
    {
        SHProbeL2 probe = {};
        return probe;
    }
};

static const float L0_NORMALIZATION = 0.5 * sqrt(1.0 / float.getPi());
static const float L1_NORMALIZATION = 0.5 * sqrt(3.0 / float.getPi());
static const float L2_2_NORMALIZATION = 0.5 * sqrt(15.0 / float.getPi());
static const float L20_NORMALIZATION = 0.25 * sqrt(5.0 / float.getPi());
static const float L22_NORMALIZATION = 0.25 * sqrt(15.0 / float.getPi());

static const float A_HAT_0 = float.getPi();
static const float A_HAT_1 = 2.0 * float.getPi() / 3.0;
static const float A_HAT_2 = float.getPi() / 4.0;

float Y0()
{
    return L0_NORMALIZATION;
}

float Y1_1(float3 dir)
{
    return L1_NORMALIZATION * dir.y;
}

float Y10(float3 dir)
{
    return L1_NORMALIZATION * dir.z;
}

float Y11(float3 dir)
{
    return L1_NORMALIZATION * dir.x;
}

float Y2_2(float3 dir)
{
    return L2_2_NORMALIZATION * dir.x * dir.y;
}

float Y2_1(float3 dir)
{
    return L2_2_NORMALIZATION * dir.y * dir.z;
}

float Y20(float3 dir)
{
    return L20_NORMALIZATION * (3.0 * dir.z * dir.z - 1);
}

float Y21(float3 dir)
{
    return L2_2_NORMALIZATION * dir.x * dir.z;
}

float Y22(float3 dir)
{
    return L22_NORMALIZATION * (dir.x * dir.x - dir.y * dir.y);
}

// https://www.ppsloan.org/publications/shdering.pdf
float SHLanczos(float l, float w)
{
    float v = float.getPi() * l / w;
    return pow(sin(v) / v, 4.0);
}

void DeringSHProbeL2(inout SHProbeL2 probe, float w)
{
    float w1 = SHLanczos(1.0, w);
    float w2 = SHLanczos(2.0, w);

    probe.sh[1].xyz *= w1;
    probe.sh[2].xyz *= w1;
    probe.sh[3].xyz *= w1;

    probe.sh[4].xyz *= w2;
    probe.sh[5].xyz *= w2;
    probe.sh[6].xyz *= w2;
    probe.sh[7].xyz *= w2;
    probe.sh[8].xyz *= w2;
}
