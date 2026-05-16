#pragma once

// [0.0, 1.0)
float UintToFloatExclusive(const uint x)
{
    return (float)x * (1.0f / 4294967808.0f);
}

// [0.0, 1.0]
float UintToFloatInclusive(const uint x)
{
    return (float)x * (1.0f / (float)0xFFFFFFFFu);
}

uint CountLeadingZeros(uint x)
{
    return 31 - firstbithigh(x);
}

uint NextPowerOfTwo(const uint x)
{
    return x == 0 ? 1 : 1 << (32 - CountLeadingZeros(x));
}

// https://github.com/skeeto/hash-prospector
uint HashHPUint(uint i)
{
    i ^= i >> 16;
    i *= 0x21f0aaad;
    i ^= i >> 15;
    i *= 0xd35a2d97;
    i ^= i >> 15;
    return i ^ 0xe6fe3beb;
}

uint3 HashPcg3d(uint3 v)
{
    v = v * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v = v ^ (v >> 16u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return v;
}

uint Part1By1(uint x)
{
    x &= 0x0000ffff;
    x = (x ^ (x << 8)) & 0x00ff00ff;
    x = (x ^ (x << 4)) & 0x0f0f0f0f;
    x = (x ^ (x << 2)) & 0x33333333;
    x = (x ^ (x << 1)) & 0x55555555;
    return x;
}

uint MortonEncode(uint x, uint y)
{
    return Part1By1(x) | (Part1By1(y) << 1);
}
