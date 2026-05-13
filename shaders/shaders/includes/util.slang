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
