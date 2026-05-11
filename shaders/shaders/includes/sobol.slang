// Sobol-Burley sampler — Slang/HLSL
// Translated from github.com/cessen/sobol_burley (MIT / Apache-2.0)

[[vk::binding(9, 0)]]
StructuredBuffer<uint4> SobolVectors;

uint sb_hash(uint n)
{
    n ^= 0xe6fe3bebu;
    n ^= n >> 16;
    n *= 0x7feb352du;
    n ^= n >> 15;
    n *= 0x846ca68bu;
    n ^= n >> 16;
    return n;
}

uint sb_owen_scramble_rev(uint n_rev, uint scramble)
{
    n_rev ^= n_rev * 0x3d20adeau;
    n_rev += scramble;
    n_rev *= (scramble >> 16) | 1u;
    n_rev ^= n_rev * 0x05526c56u;
    n_rev ^= n_rev * 0x53a22864u;
    return n_rev;
}

float sb_u32_to_f32(uint n)
{
    return asfloat((n >> 9) | 0x3f800000u) - 1.0;
}

uint sb_sobol_rev(uint sample_index_rev, uint dimension)
{
    uint dimension_set = dimension >> 2;
    uint sub_dimension  = dimension & 0x3u;
    uint base_idx = dimension_set << 4;

    uint sobol = 0u;
    uint index = sample_index_rev & 0xffff0000u;
    uint i = 0u;
    while (index != 0u)
    {
        uint j = 31u - firstbithigh(index);
        sobol ^= SobolVectors[base_idx + i + j][sub_dimension];
        i += j + 1u;
        index <<= j;
        index <<= 1u;
    }
    return sobol;
}

float sb_sample(uint sample_index, uint dimension, uint seed)
{
    uint shuffled_rev = sb_owen_scramble_rev(
        reversebits(sample_index),
        sb_hash(seed ^ 0x79c68e4au)
    );

    uint sobol = sb_sobol_rev(shuffled_rev, dimension);

    // Build per-dimension scramble seed
    uint s  = seed * 0x9c8f2d3bu;
    uint ds = dimension >> 2;
    static const uint offsets[4] = { 0x912f69bau, 0x174f18abu, 0x691e72cau, 0xb40cc1b8u };
    uint scramble = ds ^ s ^ offsets[dimension & 0x3u];

    uint sobol_owen_rev = sb_owen_scramble_rev(sobol, sb_hash(scramble));

    return sb_u32_to_f32(reversebits(sobol_owen_rev));
}

uint SobolPixelSeed(uint2 coords)
{
    return sb_hash(coords.x ^ (coords.y * 0x9e3779b9u));
}

float2 GetSampleSobol(uint sampleIndex, uint dimension, uint pixelSeed)
{
    float x = sb_sample(sampleIndex, dimension * 2u + 0u, pixelSeed);
    float y = sb_sample(sampleIndex, dimension * 2u + 1u, pixelSeed);
    return float2(x, y);
}
