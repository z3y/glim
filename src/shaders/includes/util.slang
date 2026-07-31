// SPDX-FileCopyrightText: 2011-2022 Blender Foundation
// SPDX-FileCopyrightText: 2026 z3y
// SPDX-License-Identifier: Apache-2.0
//
// Adapted code from Blender Foundation

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
    i ^= i >> 16u;
    i *= 0x21f0aaad;
    i ^= i >> 15u;
    i *= 0xd35a2d97;
    i ^= i >> 15u;
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

#define rot(x, k) (((x) << (k)) | ((x) >> (32 - (k))))

#define final(a, b, c)   \
    {                    \
        c ^= b;          \
        c -= rot(b, 14); \
        a ^= c;          \
        a -= rot(c, 11); \
        b ^= a;          \
        b -= rot(a, 25); \
        c ^= b;          \
        c -= rot(b, 16); \
        a ^= c;          \
        a -= rot(c, 4);  \
        b ^= a;          \
        b -= rot(a, 14); \
        c ^= b;          \
        c -= rot(b, 24); \
    }

uint hash_uint(const uint kx)
{
    uint a;
    uint b;
    uint c;
    a = b = c = 0xdeadbeef + (1 << 2) + 13;

    a += kx;
    final(a, b, c);

    return c;
}

uint hash_uint2(const uint kx, const uint ky)
{
    uint a;
    uint b;
    uint c;
    a = b = c = 0xdeadbeef + (2 << 2) + 13;

    b += ky;
    a += kx;
    final(a, b, c);

    return c;
}

uint InterleaveWithZero(uint word)
{
    word = (word ^ (word << 8)) & 0x00ff00ff;
    word = (word ^ (word << 4)) & 0x0f0f0f0f;
    word = (word ^ (word << 2)) & 0x33333333;
    word = (word ^ (word << 1)) & 0x55555555;
    return word;
}

uint DeinterleaveWithZero(uint word)
{
    word &= 0x55555555;
    word = (word | (word >> 1)) & 0x33333333;
    word = (word | (word >> 2)) & 0x0f0f0f0f;
    word = (word | (word >> 4)) & 0x00ff00ff;
    word = (word | (word >> 8)) & 0x0000ffff;
    return word;
}

uint2 IndexToUV(uint index)
{
    return uint2(DeinterleaveWithZero(index), DeinterleaveWithZero(index >> 1));
}
