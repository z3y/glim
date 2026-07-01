// SPDX-FileCopyrightText: 2011-2022 Blender Foundation
// SPDX-FileCopyrightText: 2026 z3y
// SPDX-License-Identifier: Apache-2.0
//
// Adapted code from Blender Foundation

#pragma once

#include "util.slang"

/* clang-format off */
static const uint SobolBurleyTable[4][32] =
{
    {
        0x00000001, 0x00000002, 0x00000004, 0x00000008,
        0x00000010, 0x00000020, 0x00000040, 0x00000080,
        0x00000100, 0x00000200, 0x00000400, 0x00000800,
        0x00001000, 0x00002000, 0x00004000, 0x00008000,
        0x00010000, 0x00020000, 0x00040000, 0x00080000,
        0x00100000, 0x00200000, 0x00400000, 0x00800000,
        0x01000000, 0x02000000, 0x04000000, 0x08000000,
        0x10000000, 0x20000000, 0x40000000, 0x80000000,
    },
    {
        0x00000001, 0x00000003, 0x00000005, 0x0000000f,
        0x00000011, 0x00000033, 0x00000055, 0x000000ff,
        0x00000101, 0x00000303, 0x00000505, 0x00000f0f,
        0x00001111, 0x00003333, 0x00005555, 0x0000ffff,
        0x00010001, 0x00030003, 0x00050005, 0x000f000f,
        0x00110011, 0x00330033, 0x00550055, 0x00ff00ff,
        0x01010101, 0x03030303, 0x05050505, 0x0f0f0f0f,
        0x11111111, 0x33333333, 0x55555555, 0xffffffff,
    },
    {
        0x00000001, 0x00000003, 0x00000006, 0x00000009,
        0x00000017, 0x0000003a, 0x00000071, 0x000000a3,
        0x00000116, 0x00000339, 0x00000677, 0x000009aa,
        0x00001601, 0x00003903, 0x00007706, 0x0000aa09,
        0x00010117, 0x0003033a, 0x00060671, 0x000909a3,
        0x00171616, 0x003a3939, 0x00717777, 0x00a3aaaa,
        0x01170001, 0x033a0003, 0x06710006, 0x09a30009,
        0x16160017, 0x3939003a, 0x77770071, 0xaaaa00a3,
    },
    {
        0x00000001, 0x00000003, 0x00000004, 0x0000000a,
        0x0000001f, 0x0000002e, 0x00000045, 0x000000c9,
        0x0000011b, 0x000002a4, 0x0000079a, 0x00000b67,
        0x0000101e, 0x0000302d, 0x00004041, 0x0000a0c3,
        0x0001f104, 0x0002e28a, 0x000457df, 0x000c9bae,
        0x0011a105, 0x002a7289, 0x0079e7db, 0x00b6dba4,
        0x0100011a, 0x030002a7, 0x0400079e, 0x0a000b6d,
        0x1f001001, 0x2e003003, 0x45004004, 0xc900a00a,
    },
};
/* clang-format on */

// A shuffled, Owen-scrambled Sobol sampler, implemented with the
// techniques from the paper "Practical Hash-based Owen Scrambling"
// by Brent Burley, 2020, Journal of Computer Graphics Techniques.
//
// Note that unlike a standard high-dimensional Sobol sequence, this
// Sobol sampler uses padding to achieve higher dimensions, as described
// in Burley's paper.

// [0, uint_max] -> [0.0, 1.0)

// Performs base-2 Owen scrambling on a reversed-bit unsigned integer.
//
// This is equivalent to the Laine-Karras permutation, but much higher
// quality. See https://psychopath.io/post/2021_01_30_building_a_better_lk_hash
uint ReversedBitOwen(uint n, uint seed)
{
    n ^= n * 0x3d20adea;
    n += seed;
    n *= (seed >> 16) | 1;
    n ^= n * 0x05526c56;
    n ^= n * 0x53a22864;
    return n;
}

float SobolBurley(uint revBitIndex, uint dimension, uint scrambleSeed)
{
    uint result = 0;

    if (dimension == 0)
    {
        // Fast-path for dimension 0, which is just Van der corput.
        // This makes a notable difference in performance since we reuse
        // dimensions for padding, and dimension 0 is reused the most.
        result = reversebits(revBitIndex);
    }
    else
    {
        uint i = 0;
        while (revBitIndex != 0)
        {
            uint j = CountLeadingZeros(revBitIndex);
            result ^= SobolBurleyTable[dimension][i + j];
            i += j + 1;

            // We can't do `<<= j + 1` because that can overflow the shift
            // operator, which doesn't do what we need, at least on x86.
            revBitIndex <<= j;
            revBitIndex <<= 1;
        }
    }

    // Apply Owen scrambling.
    result = reversebits(ReversedBitOwen(result, scrambleSeed));

    return UintToFloatExclusive(result);
}

float2 SobolBurleySample2D(uint index, uint dimensionSet, uint seed, uint shuffledIndexMask)
{
    // Include the dimension set in the seed, so we get decorrelated
    // sequences for different dimension sets via shuffling.
    seed ^= HashHPUint(dimensionSet);

    // Shuffle and mask. The masking is just for better
    // performance at low sample counts.
    index &= shuffledIndexMask;
    index = ReversedBitOwen(reversebits(index), seed ^ 0xf8ade99a);

    return float2(SobolBurley(index, 0, seed ^ 0xe0aaaf76),
                  SobolBurley(index, 1, seed ^ 0x94964d4e));
}

enum SamplingPattern
{
    SobolBurley = 0,
    BlueNoisePure = 1,
    BlueNoiseFirst = 2,
};

// Performs base-4 Owen scrambling on a reversed-bit unsigned integer.
// See https://psychopath.io/post/2022_08_14_a_fast_hash_for_base_4_owen_scrambling
uint reversed_bit_owen_base4(uint n, const uint seed)
{
    n ^= n * 0x3d20adea;
    n ^= (n >> 1) & (n << 1) & 0x55555555;
    n += seed;
    n *= (seed >> 16) | 1;
    n ^= (n >> 1) & (n << 1) & 0x55555555;
    n ^= n * 0x05526c56;
    n ^= n * 0x53a22864;

    return n;
}

uint nested_uniform_scramble_base4(const uint i, const uint seed)
{
    return reversebits(reversed_bit_owen_base4(reversebits(i), seed));
}

// 2D hash recommended from "Hash Functions for GPU Rendering" JCGT Vol. 9, No. 3, 2020
// See https://www.shadertoy.com/view/4tXyWN and https://www.shadertoy.com/view/XlGcRh
// http://www.jcgt.org/published/0009/03/02/paper.pdf
uint hash_iqnt2d(const uint x, const uint y)
{
    const uint qx = 1103515245U * ((x >> 1U) ^ (y));
    const uint qy = 1103515245U * ((y >> 1U) ^ (x));
    const uint n = 1103515245U * ((qx) ^ (qy >> 3U));

    return n;
}

uint RNGPixelInit(uint2 coords, SamplingPattern pattern)
{
    uint seed = 0;

    if (pattern == SamplingPattern::SobolBurley)
    {
        // The white-noise samplers use a random per-pixel hash to generate independent sequences
        return hash_iqnt2d(coords.x, coords.y) ^ seed;
    }

    seed = hash_uint(seed);
    // The blue-noise samplers use a single sequence for all pixels, but offset the index within
    // the sequence for each pixel. We use a hierarchically shuffled 2D morton curve to determine
    // each pixel's offset along the sequence.
    //
    // Based on:
    // https://psychopath.io/post/2022_07_24_owen_scrambling_based_dithered_blue_noise_sampling.
    return nested_uniform_scramble_base4(MortonEncode(coords.x, coords.y), seed);
}

uint3 BlueNoiseIndexing(uint pixelIndex,
                        uint sample,
                        uint sobolIndexMask,
                        uint blueNoiseSequenceLength,
                        SamplingPattern pattern)
{
    if (pattern == SamplingPattern::SobolBurley)
    {
        return uint3(sample, pixelIndex, sobolIndexMask);
    }
    else if (pattern == SamplingPattern::BlueNoisePure)
    {
        uint globalSampleIndex = sample + (pixelIndex * blueNoiseSequenceLength);
        return uint3(globalSampleIndex, 0, 0xffffffff);
    }
    else // SamplingPattern::BlueNoiseFirst
    {
        if (sample == 0)
        {
            return uint3(pixelIndex, 0x0cd0519f, 0xffffffff);
        }
        uint globalSampleIndex = (sample - 1) + (pixelIndex * blueNoiseSequenceLength);
        return uint3(globalSampleIndex, 0, 0xffffffff);
    }
}
