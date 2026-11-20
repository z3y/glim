#ifndef GLIM_NEURAL_LIGHTMAP_INCLUDED
#define GLIM_NEURAL_LIGHTMAP_INCLUDED

// Glim Neural Lightmaps (Lightmap Mode: Neural)
//
// Every lightmap pixel has 16 features (_GlimNeural0..3, RGBA half, same resolution as the lightmap)
// and one small MLP is shared by the scene (_GlimNeuralWeights, RGBA float, 64 texels per row).
// The textures are bound per renderer by the Glim Neural Lightmaps component.
//
//   radiance = MLP(features, SH4(direction), lobe)
//
// lobe GLIM_NN_LOBE_COSINE: diffuse for a normal, the same units as the regular lightmap, works with normal maps
// lobe from GlimNNSpecularLobe: incoming radiance prefiltered for the GGX lobe, like a reflection probe sample
// Directions are in world space.
//
// Must match shaders/includes/neural.slang

#define GLIM_NN_FEATURES 16
#define GLIM_NN_IN 48
#define GLIM_NN_HIDDEN 64

// offsets in float4 texels
#define GLIM_NN_W1 0
#define GLIM_NN_B1 768
#define GLIM_NN_W2 784
#define GLIM_NN_B2 1808
#define GLIM_NN_W3 1824
#define GLIM_NN_B3 1888
#define GLIM_NN_WEIGHTS_WIDTH 64

#define GLIM_NN_SHARPNESS_MIN 2.0
#define GLIM_NN_SHARPNESS_MAX 4096.0
#define GLIM_NN_LEAKY_SLOPE 0.01
#define GLIM_NN_LOBE_COSINE 0.0

Texture2D<float4> _GlimNeuralWeights;
Texture2D<float4> _GlimNeural0;
Texture2D<float4> _GlimNeural1;
Texture2D<float4> _GlimNeural2;
Texture2D<float4> _GlimNeural3;

struct GlimNNFeatures
{
    float4 f[4];
};

float4 GlimNNWeight(uint texel)
{
    return _GlimNeuralWeights.Load(int3(texel % GLIM_NN_WEIGHTS_WIDTH, texel / GLIM_NN_WEIGHTS_WIDTH, 0));
}

GlimNNFeatures GlimNNLoadFeatures(int2 texel)
{
    GlimNNFeatures features;
    features.f[0] = _GlimNeural0.Load(int3(texel, 0));
    features.f[1] = _GlimNeural1.Load(int3(texel, 0));
    features.f[2] = _GlimNeural2.Load(int3(texel, 0));
    features.f[3] = _GlimNeural3.Load(int3(texel, 0));
    return features;
}

GlimNNFeatures GlimNNLerpFeatures(GlimNNFeatures a, GlimNNFeatures b, float t)
{
    GlimNNFeatures r;
    [unroll]
    for (int i = 0; i < 4; i++)
    {
        r.f[i] = lerp(a.f[i], b.f[i], t);
    }
    return r;
}

// lobe code for specular, from the perceptual roughness and the view angle (warped GGX as a SG, see the training)
float GlimNNSpecularLobe(float perceptualRoughness, float NoV)
{
    float alpha = max(perceptualRoughness * perceptualRoughness, 0.01);
    float sharpness = (2.0 / (alpha * alpha)) / (4.0 * max(NoV, 1e-3));
    float t = saturate((log2(sharpness) - log2(GLIM_NN_SHARPNESS_MIN)) / (log2(GLIM_NN_SHARPNESS_MAX) - log2(GLIM_NN_SHARPNESS_MIN)));
    return (1.0 + 6.0 * t) / 7.0;
}

// the inputs after the features: SH up to band 4 of the direction (25) and the one blob lobe encoding (7)
void GlimNNEncode(float3 d, float lobe, out float4 e[8])
{
    float x = d.x, y = d.y, z = d.z;
    float x2 = x * x, y2 = y * y, z2 = z * z;
    float xy = x * y, yz = y * z, xz = x * z;

    float v[32];
    v[0] = 0.28209479177387814;
    v[1] = -0.48860251190291987 * y;
    v[2] = 0.48860251190291987 * z;
    v[3] = -0.48860251190291987 * x;
    v[4] = 1.0925484305920792 * xy;
    v[5] = -1.0925484305920792 * yz;
    v[6] = 0.94617469575755997 * z2 - 0.31539156525251999;
    v[7] = -1.0925484305920792 * xz;
    v[8] = 0.54627421529603959 * x2 - 0.54627421529603959 * y2;
    v[9] = 0.59004358992664352 * y * (-3.0 * x2 + y2);
    v[10] = 2.8906114426405538 * xy * z;
    v[11] = 0.45704579946446572 * y * (1.0 - 5.0 * z2);
    v[12] = 0.3731763325901154 * z * (5.0 * z2 - 3.0);
    v[13] = 0.45704579946446572 * x * (1.0 - 5.0 * z2);
    v[14] = 1.4453057213202769 * z * (x2 - y2);
    v[15] = 0.59004358992664352 * x * (-x2 + 3.0 * y2);
    v[16] = 2.5033429417967046 * xy * (x2 - y2);
    v[17] = 1.7701307697799304 * yz * (-3.0 * x2 + y2);
    v[18] = 0.94617469575756008 * xy * (7.0 * z2 - 1.0);
    v[19] = 0.66904654355728921 * yz * (3.0 - 7.0 * z2);
    v[20] = -3.1735664074561294 * z2 + 3.7024941420321507 * z2 * z2 + 0.31735664074561293;
    v[21] = 0.66904654355728921 * xz * (3.0 - 7.0 * z2);
    v[22] = 0.47308734787878004 * (x2 - y2) * (7.0 * z2 - 1.0);
    v[23] = 1.7701307697799304 * xz * (-x2 + 3.0 * y2);
    v[24] = -3.7550144126950569 * x2 * y2 + 0.62583573544917614 * x2 * x2 + 0.62583573544917614 * y2 * y2;

    [unroll]
    for (int k = 0; k < 7; k++)
    {
        float t = (lobe - k / 6.0) * 6.0;
        v[25 + k] = exp(-0.5 * t * t);
    }

    [unroll]
    for (int i = 0; i < 8; i++)
    {
        e[i] = float4(v[i * 4 + 0], v[i * 4 + 1], v[i * 4 + 2], v[i * 4 + 3]);
    }
}

float GlimNNActivation(float v)
{
    return v > 0.0 ? v : v * GLIM_NN_LEAKY_SLOPE;
}

float3 GlimNNEvaluateEncoded(GlimNNFeatures features, float4 encoded[8])
{
    float4 x[12];
    x[0] = features.f[0];
    x[1] = features.f[1];
    x[2] = features.f[2];
    x[3] = features.f[3];
    [unroll]
    for (int e = 0; e < 8; e++)
    {
        x[4 + e] = encoded[e];
    }

    float4 h1[16];
    [loop]
    for (uint i4 = 0; i4 < 16; i4++)
    {
        float4 sum = GlimNNWeight(GLIM_NN_B1 + i4);
        [unroll]
        for (uint c = 0; c < 4; c++)
        {
            uint row = GLIM_NN_W1 + (i4 * 4 + c) * 12;
            float s = 0.0;
            [unroll]
            for (uint k = 0; k < 12; k++)
            {
                s += dot(GlimNNWeight(row + k), x[k]);
            }
            sum[c] += s;
        }
        h1[i4] = float4(GlimNNActivation(sum.x), GlimNNActivation(sum.y), GlimNNActivation(sum.z), GlimNNActivation(sum.w));
    }

    float4 h2[16];
    [loop]
    for (uint j4 = 0; j4 < 16; j4++)
    {
        float4 sum = GlimNNWeight(GLIM_NN_B2 + j4);
        [unroll]
        for (uint c = 0; c < 4; c++)
        {
            uint row = GLIM_NN_W2 + (j4 * 4 + c) * 16;
            float s = 0.0;
            [unroll]
            for (uint k = 0; k < 16; k++)
            {
                s += dot(GlimNNWeight(row + k), h1[k]);
            }
            sum[c] += s;
        }
        h2[j4] = float4(GlimNNActivation(sum.x), GlimNNActivation(sum.y), GlimNNActivation(sum.z), GlimNNActivation(sum.w));
    }

    float3 z = GlimNNWeight(GLIM_NN_B3).xyz;
    [unroll]
    for (uint c = 0; c < 3; c++)
    {
        uint row = GLIM_NN_W3 + c * 16;
        float s = 0.0;
        [unroll]
        for (uint k = 0; k < 16; k++)
        {
            s += dot(GlimNNWeight(row + k), h2[k]);
        }
        z[c] += s;
    }

    return exp(min(z, 15.0));
}

float3 GlimNNEvaluate(GlimNNFeatures features, float3 direction, float lobe)
{
    float4 encoded[8];
    GlimNNEncode(direction, lobe, encoded);
    return GlimNNEvaluateEncoded(features, encoded);
}

struct GlimNNTaps
{
    int2 texel;
    float2 weight;
};

// lightmapUV is uv1 * unity_LightmapST.xy + unity_LightmapST.zw
GlimNNTaps GlimNNGetTaps(float2 lightmapUV)
{
    float width, height;
    _GlimNeural0.GetDimensions(width, height);
    float2 p = lightmapUV * float2(width, height) - 0.5;
    GlimNNTaps taps;
    taps.texel = int2(floor(p));
    taps.weight = p - floor(p);
    return taps;
}

GlimNNFeatures GlimNNLoadTap(GlimNNTaps taps, int2 offset)
{
    float width, height;
    _GlimNeural0.GetDimensions(width, height);
    int2 texel = clamp(taps.texel + offset, int2(0, 0), int2(width, height) - 1);
    return GlimNNLoadFeatures(texel);
}

// the network evaluated at the 4 nearest pixels and blended, the same as the training sees it (4x the cost)
float3 GlimNNSampleBlended(float2 lightmapUV, float3 direction, float lobe)
{
    GlimNNTaps taps = GlimNNGetTaps(lightmapUV);
    float4 encoded[8];
    GlimNNEncode(direction, lobe, encoded);

    float3 result = 0.0;
    [loop]
    for (int tap = 0; tap < 4; tap++)
    {
        int2 offset = int2(tap & 1, tap >> 1);
        float wx = offset.x == 1 ? taps.weight.x : 1.0 - taps.weight.x;
        float wy = offset.y == 1 ? taps.weight.y : 1.0 - taps.weight.y;
        result += GlimNNEvaluateEncoded(GlimNNLoadTap(taps, offset), encoded) * (wx * wy);
    }
    return result;
}

// bilinear filtered features and one network evaluation, cheaper but the features were trained per pixel
float3 GlimNNSampleBilinearFeatures(float2 lightmapUV, float3 direction, float lobe)
{
    GlimNNTaps taps = GlimNNGetTaps(lightmapUV);
    GlimNNFeatures f0 = GlimNNLerpFeatures(GlimNNLoadTap(taps, int2(0, 0)), GlimNNLoadTap(taps, int2(1, 0)), taps.weight.x);
    GlimNNFeatures f1 = GlimNNLerpFeatures(GlimNNLoadTap(taps, int2(0, 1)), GlimNNLoadTap(taps, int2(1, 1)), taps.weight.x);
    return GlimNNEvaluate(GlimNNLerpFeatures(f0, f1, taps.weight.y), direction, lobe);
}

#endif
