#pragma once

// Spherical Gaussian lightmaps
//
// 8 fixed lobes on the corners of a cube in Unity world space, the same for every texel,
// so bilinear filtering of the amplitudes is exact.
// Must match SG_AXES / SG_SHARPNESS in src/sg.rs and GlimSGLightmap.hlsl.
//
// The bake projects the radiance onto the lobes: b_i = sum(pi * contribution * G_i(dir)) / sampleCount,
// where contribution is the same value that is added to the diffuse lightmap.
// With cosine weighted samples this estimates the cosine weighted projection
// b_i = integral(max(dot(n, w), 0) * G_i(w) * L(w) dw), solved on the CPU per texel
// for the amplitudes of a cosine weighted least squares fit.

static const uint SG_LOBE_COUNT = 8;
// broad lobes, sharper ones lose too much in the directions between the cube corners, where axis aligned
// normals point (fit error of a constant sky on a floor is 11% at 3.0 and 15% at 4.0, 23% too dark at the zenith)
static const float SG_SHARPNESS = 3.0;

struct SGProjection
{
    float3 b[SG_LOBE_COUNT];

    static SGProjection zero()
    {
        SGProjection p = {};
        return p;
    }
};

float3 SGAxis(uint lobe)
{
    const float s = 0.57735026919; // 1 / sqrt(3)
    return float3((lobe & 1u) != 0u ? s : -s,
                  (lobe & 2u) != 0u ? s : -s,
                  (lobe & 4u) != 0u ? s : -s);
}

// dir is in bake space, the axes are in Unity space
void AccumulateSG(inout SGProjection projection, float3 contribution, float3 dir)
{
    // flip Z for Unity
    dir.z = -dir.z;

    float3 scaled = contribution * float.getPi();

    [unroll]
    for (uint lobe = 0; lobe < SG_LOBE_COUNT; lobe++)
    {
        float weight = exp(SG_SHARPNESS * (dot(dir, SGAxis(lobe)) - 1.0));
        projection.b[lobe] += scaled * weight;
    }
}

// lobe k is stored after the diffuse block at (index + texelCount * (1 + k)) * 3
void WriteSG(float *lightmap, uint index, uint texelCount, SGProjection projection, float sampleScale)
{
    [unroll]
    for (uint lobe = 0; lobe < SG_LOBE_COUNT; lobe++)
    {
        float3 value = projection.b[lobe] * sampleScale;
        if (!all(isfinite(value)))
        {
            continue;
        }
        uint i = index + texelCount * (1 + lobe);
        lightmap[i * 3 + 0] += value.x;
        lightmap[i * 3 + 1] += value.y;
        lightmap[i * 3 + 2] += value.z;
    }
}
