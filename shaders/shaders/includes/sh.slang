// SPDX-License-Identifier: MIT
// Author: pema99
// https://gist.github.com/pema99/226022f4e78173a2c21c56044e86d4a4

#pragma once

struct SHProbeL2
{
    float3 position;
    float pad0;
    float4 sh[9];
};

static const float L0_NORMALIZATION = 0.5 * sqrt(1.0 / float.getPi());
static const float L1_NORMALIZATION = 0.5 * sqrt(3.0 / float.getPi());
static const float L2_2_NORMALIZATION = 0.5 * sqrt(15.0 / float.getPi());
static const float L20_NORMALIZATION = 0.25 * sqrt(5.0 / float.getPi());
static const float L22_NORMALIZATION = 0.25 * sqrt(15.0 / float.getPi());

static const float A_HAT_0 = float.getPi();
static const float A_HAT_1 = 2.0 * float.getPi() / 3.0;
static const float A_HAT_2 = float.getPi() / 4.0;

// SH basis functions
float Y0() // Basis function l=0
{
    return L0_NORMALIZATION;
}

float Y1_1(float3 dir) // Basis function l=1, m=-1
{
    return L1_NORMALIZATION * dir.y;
}

float Y10(float3 dir) // Basis function l=1, m=0
{
    return L1_NORMALIZATION * dir.z;
}

float Y11(float3 dir) // Basis function l=1, m=1
{
    return L1_NORMALIZATION * dir.x;
}

float Y2_2(float3 dir) // Basis function l=2, m=-2
{
    return L2_2_NORMALIZATION * dir.x * dir.y;
}

float Y2_1(float3 dir) // Basis function l=2, m=-1
{
    return L2_2_NORMALIZATION * dir.y * dir.z;
}

float Y20(float3 dir) // Basis function l=2, m=0
{
    return L20_NORMALIZATION * (3.0 * dir.z * dir.z - 1);
}

float Y21(float3 dir) // Basis function l=2, m=1
{
    return L2_2_NORMALIZATION * dir.x * dir.z;
}

float Y22(float3 dir) // Basis function l=2, m=2
{
    return L22_NORMALIZATION * (dir.x * dir.x - dir.y * dir.y);
}
