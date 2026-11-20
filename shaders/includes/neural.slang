#pragma once

#include "util.slang"

// Neural lightmaps
//
// Every lightmap pixel has NN_FEATURES latent features, one small MLP is shared by the whole scene:
//
//     radiance = MLP(features, encode(direction), encode(lobe))
//
// The output is the incoming radiance at the pixel, prefiltered with a lobe around a direction:
//   lobe code 0           clamped cosine around the direction, normalized: (1/pi) * integral(max(dot(n, w), 0) * L(w) dw)
//                         with the normal as direction this is the regular diffuse lightmap value, works with normal maps
//   lobe code (1/7, 1]    normalized Spherical Gaussian around the direction, sharpness from NN_SHARPNESS_MIN to NN_SHARPNESS_MAX
//                         prefiltered radiance for specular, query with the reflection vector
// Directions are in Unity world space.
//
// Must match src/neural.rs and unity/ShaderLibrary/GlimNeuralLightmap.hlsl

static const uint NN_FEATURES = 16;
static const uint NN_DIR_ENCODING = 25; // SH up to band 4
static const uint NN_LOBE_ENCODING = 7; // one blob
static const uint NN_IN = NN_FEATURES + NN_DIR_ENCODING + NN_LOBE_ENCODING; // 48
static const uint NN_HIDDEN = 64;
static const uint NN_OUT = 4; // rgb, padded

static const uint NN_W1 = 0;
static const uint NN_B1 = NN_W1 + NN_HIDDEN * NN_IN;
static const uint NN_W2 = NN_B1 + NN_HIDDEN;
static const uint NN_B2 = NN_W2 + NN_HIDDEN * NN_HIDDEN;
static const uint NN_W3 = NN_B2 + NN_HIDDEN;
static const uint NN_B3 = NN_W3 + NN_OUT * NN_HIDDEN;
static const uint NN_WEIGHT_COUNT = NN_B3 + NN_OUT; // 7556

static const float NN_SHARPNESS_MIN = 2.0;
static const float NN_SHARPNESS_MAX = 4096.0;
static const float NN_LEAKY_SLOPE = 0.01;
static const float NN_MAX_LOG = 15.0;

// per training sample: inputs, both hidden activations, and the deltas of every layer
static const uint NN_BATCH_X = 0;
static const uint NN_BATCH_H1 = NN_BATCH_X + NN_IN;
static const uint NN_BATCH_H2 = NN_BATCH_H1 + NN_HIDDEN;
static const uint NN_BATCH_D1 = NN_BATCH_H2 + NN_HIDDEN;
static const uint NN_BATCH_D2 = NN_BATCH_D1 + NN_HIDDEN;
static const uint NN_BATCH_D3 = NN_BATCH_D2 + NN_HIDDEN;
static const uint NN_BATCH_STRIDE = NN_BATCH_D3 + NN_OUT; // 308

static const float NN_LOBE_COSINE = 0.0;

float NNLobeCodeSG(float t)
{
    return (1.0 + 6.0 * saturate(t)) / 7.0;
}

float NNSharpness(float t)
{
    return exp2(lerp(log2(NN_SHARPNESS_MIN), log2(NN_SHARPNESS_MAX), saturate(t)));
}

float NNSharpnessT(float sharpness)
{
    return saturate((log2(sharpness) - log2(NN_SHARPNESS_MIN)) / (log2(NN_SHARPNESS_MAX) - log2(NN_SHARPNESS_MIN)));
}

// normalized SG, integrates to 1 over the sphere
float NNSGNormalized(float3 axis, float sharpness, float3 dir)
{
    float norm = sharpness / (2.0 * float.getPi() * (1.0 - exp(-2.0 * sharpness)));
    return norm * exp(sharpness * (dot(axis, dir) - 1.0));
}

float3 NNTangentToWorld(float3 local, float3 axis)
{
    float3 up = abs(axis.z) < 0.999 ? float3(0, 0, 1) : float3(1, 0, 0);
    float3 t = normalize(cross(up, axis));
    float3 b = cross(axis, t);
    return t * local.x + b * local.y + axis * local.z;
}

// exact sample of a normalized SG
float3 NNSampleSG(float3 axis, float sharpness, float2 xi)
{
    float cosTheta = 1.0 + log(xi.x + (1.0 - xi.x) * exp(-2.0 * sharpness)) / sharpness;
    cosTheta = clamp(cosTheta, -1.0, 1.0);
    float sinTheta = sqrt(max(0.0, 1.0 - cosTheta * cosTheta));
    float phi = 2.0 * float.getPi() * xi.y;
    return NNTangentToWorld(float3(cos(phi) * sinTheta, sin(phi) * sinTheta, cosTheta), axis);
}

float3 NNSampleCone(float3 axis, float cosMax, float2 xi)
{
    float cosTheta = lerp(1.0, cosMax, xi.x);
    float sinTheta = sqrt(max(0.0, 1.0 - cosTheta * cosTheta));
    float phi = 2.0 * float.getPi() * xi.y;
    return NNTangentToWorld(float3(cos(phi) * sinTheta, sin(phi) * sinTheta, cosTheta), axis);
}

struct NNRng
{
    uint state;

    __init(uint a, uint b)
    {
        state = HashPcg3d(uint3(a, b, 0x9E3779B9u)).x;
    }

    [mutating]
    float next()
    {
        state = state * 747796405u + 2891336453u;
        uint word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
        word = (word >> 22u) ^ word;
        return float(word >> 8) * (1.0 / 16777216.0);
    }

    [mutating]
    float2 next2()
    {
        float a = next();
        float b = next();
        return float2(a, b);
    }
};

// d in Unity space
void NNEncodeDirection(float3 d, inout float x[NN_IN])
{
    const uint o = NN_FEATURES;
    float px = d.x, py = d.y, pz = d.z;
    float x2 = px * px, y2 = py * py, z2 = pz * pz;
    float xy = px * py, yz = py * pz, xz = px * pz;

    x[o + 0] = 0.28209479177387814;
    x[o + 1] = -0.48860251190291987 * py;
    x[o + 2] = 0.48860251190291987 * pz;
    x[o + 3] = -0.48860251190291987 * px;
    x[o + 4] = 1.0925484305920792 * xy;
    x[o + 5] = -1.0925484305920792 * yz;
    x[o + 6] = 0.94617469575755997 * z2 - 0.31539156525251999;
    x[o + 7] = -1.0925484305920792 * xz;
    x[o + 8] = 0.54627421529603959 * x2 - 0.54627421529603959 * y2;
    x[o + 9] = 0.59004358992664352 * py * (-3.0 * x2 + y2);
    x[o + 10] = 2.8906114426405538 * xy * pz;
    x[o + 11] = 0.45704579946446572 * py * (1.0 - 5.0 * z2);
    x[o + 12] = 0.3731763325901154 * pz * (5.0 * z2 - 3.0);
    x[o + 13] = 0.45704579946446572 * px * (1.0 - 5.0 * z2);
    x[o + 14] = 1.4453057213202769 * pz * (x2 - y2);
    x[o + 15] = 0.59004358992664352 * px * (-x2 + 3.0 * y2);
    x[o + 16] = 2.5033429417967046 * xy * (x2 - y2);
    x[o + 17] = 1.7701307697799304 * yz * (-3.0 * x2 + y2);
    x[o + 18] = 0.94617469575756008 * xy * (7.0 * z2 - 1.0);
    x[o + 19] = 0.66904654355728921 * yz * (3.0 - 7.0 * z2);
    x[o + 20] = -3.1735664074561294 * z2 + 3.7024941420321507 * z2 * z2 + 0.31735664074561293;
    x[o + 21] = 0.66904654355728921 * xz * (3.0 - 7.0 * z2);
    x[o + 22] = 0.47308734787878004 * (x2 - y2) * (7.0 * z2 - 1.0);
    x[o + 23] = 1.7701307697799304 * xz * (-x2 + 3.0 * y2);
    x[o + 24] = -3.7550144126950569 * x2 * y2 + 0.62583573544917614 * x2 * x2 + 0.62583573544917614 * y2 * y2;
}

void NNEncodeLobe(float code, inout float x[NN_IN])
{
    const uint o = NN_FEATURES + NN_DIR_ENCODING;
    [unroll]
    for (uint k = 0; k < NN_LOBE_ENCODING; k++)
    {
        float d = (code - float(k) / float(NN_LOBE_ENCODING - 1)) * float(NN_LOBE_ENCODING - 1);
        x[o + k] = exp(-0.5 * d * d);
    }
}

float NNActivation(float v)
{
    return v > 0.0 ? v : v * NN_LEAKY_SLOPE;
}

// derivative from the activation output, the sign is the same as the input
float NNActivationDerivative(float h)
{
    return h > 0.0 ? 1.0 : NN_LEAKY_SLOPE;
}

// returns the log radiance
float3 NNForward(float *w, float x[NN_IN], out float h1[NN_HIDDEN], out float h2[NN_HIDDEN])
{
    [loop]
    for (uint i = 0; i < NN_HIDDEN; i++)
    {
        float sum = w[NN_B1 + i];
        uint row = NN_W1 + i * NN_IN;
        [unroll]
        for (uint k = 0; k < NN_IN; k++)
        {
            sum += w[row + k] * x[k];
        }
        h1[i] = NNActivation(sum);
    }

    [loop]
    for (uint j = 0; j < NN_HIDDEN; j++)
    {
        float sum = w[NN_B2 + j];
        uint row = NN_W2 + j * NN_HIDDEN;
        [unroll]
        for (uint i = 0; i < NN_HIDDEN; i++)
        {
            sum += w[row + i] * h1[i];
        }
        h2[j] = NNActivation(sum);
    }

    float3 z;
    [unroll]
    for (uint c = 0; c < 3; c++)
    {
        float sum = w[NN_B3 + c];
        uint row = NN_W3 + c * NN_HIDDEN;
        [unroll]
        for (uint j = 0; j < NN_HIDDEN; j++)
        {
            sum += w[row + j] * h2[j];
        }
        z[c] = sum;
    }
    return z;
}

float3 NNRadiance(float3 z)
{
    return exp(min(z, NN_MAX_LOG));
}
