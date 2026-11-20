#include "includes/compaction.slang"
#include "includes/core.slang"
#include "includes/sobol_burley.slang"
#include "includes/neural_target.slang"

// One training sample per thread, every pixel is used at most once per step so the
// per pixel features can be updated here without atomics.
// The weight gradients are reduced from the batch buffer by neural_grad.slang.

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint2 *compaction;
    LightmapInfo *lightmapsInfo;

    NeuralData *data;
    uint step;
    uint batchOffset;

    uint batchSize;
    uint targetRays;
    float featureLR;
    float lossScale;
};

static const float ADAM_BETA1 = 0.9;
static const float ADAM_BETA2 = 0.99;
static const float ADAM_EPSILON = 1e-10;

static const float COSINE_QUERY_PROBABILITY = 0.3;
// normal maps tilt the shading normal, diffuse is also learned for normals around the surface normal
static const float COSINE_QUERY_CONE = 0.5; // cos(60 deg)
static const float RELATIVE_LOSS_EPSILON = 0.01;
static const float GRADIENT_CLIP = 8.0;

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint tid = threadID.x;
    if (tid >= Push.batchSize)
    {
        return;
    }

    NeuralData *data = Push.data;

    uint pixelIndex = data.order[(Push.batchOffset + tid) % data.pixelCount];
    NeuralPixel pixel = data.pixels[pixelIndex];

    uint visit = data.visits[pixelIndex];
    data.visits[pixelIndex] = visit + 1;

    NNRng rng = NNRng(pixelIndex, Push.step);

    NeuralSurface surface = NeuralCreateSurface(data, pixel, visit, rng.next());

    // query
    bool cosine = rng.next() < COSINE_QUERY_PROBABILITY;
    float3 direction;
    float sharpness = 0.0;
    float lobeCode;
    if (cosine)
    {
        direction = NNSampleCone(surface.normalWS, COSINE_QUERY_CONE, rng.next2());
        lobeCode = NN_LOBE_COSINE;
    }
    else
    {
        // reflection vectors cover the hemisphere above the surface
        float2 xi = rng.next2();
        float cosTheta = xi.x;
        float sinTheta = sqrt(max(0.0, 1.0 - cosTheta * cosTheta));
        float phi = 2.0 * float.getPi() * xi.y;
        direction = NNTangentToWorld(float3(cos(phi) * sinTheta, sin(phi) * sinTheta, cosTheta), surface.normalWS);

        float t = rng.next();
        sharpness = NNSharpness(t);
        lobeCode = NNLobeCodeSG(t);
    }

    float3 target = NeuralTarget(data, surface, direction, cosine, sharpness, Push.targetRays, rng);
    if (!all(isfinite(target)))
    {
        target = 0.0;
    }
    target = max(target, 0.0);

    // forward
    float x[NN_IN];
    NNBuildInput(data.features, pixelIndex, direction, lobeCode, x);

    float *w = data.weights;
    float h1[NN_HIDDEN];
    float h2[NN_HIDDEN];
    float3 z = NNForward(w, x, h1, h2);
    float3 prediction = NNRadiance(z);

    // relative L2, robust to the huge range of HDR values, the prediction in the denominator is not differentiated
    float3 denominator = prediction * prediction + RELATIVE_LOSS_EPSILON;
    float3 difference = prediction - target;
    float loss = dot(difference * difference / denominator, 1.0) / 3.0;

    // d loss / d z, through exp
    float3 dz = 2.0 * difference / denominator * prediction / 3.0;
    dz = clamp(dz, -GRADIENT_CLIP, GRADIENT_CLIP) * Push.lossScale;

    // backward
    float d2[NN_HIDDEN];
    [loop]
    for (uint j = 0; j < NN_HIDDEN; j++)
    {
        float sum = 0.0;
        [unroll]
        for (uint c = 0; c < 3; c++)
        {
            sum += w[NN_W3 + c * NN_HIDDEN + j] * dz[c];
        }
        d2[j] = sum * NNActivationDerivative(h2[j]);
    }

    float d1[NN_HIDDEN];
    [loop]
    for (uint i = 0; i < NN_HIDDEN; i++)
    {
        float sum = 0.0;
        [loop]
        for (uint j = 0; j < NN_HIDDEN; j++)
        {
            sum += w[NN_W2 + j * NN_HIDDEN + i] * d2[j];
        }
        d1[i] = sum * NNActivationDerivative(h1[i]);
    }

    // store for the weight gradients
    float *batch = data.batch + tid * NN_BATCH_STRIDE;
    [unroll]
    for (uint k = 0; k < NN_IN; k++)
    {
        batch[NN_BATCH_X + k] = x[k];
    }
    [loop]
    for (uint i = 0; i < NN_HIDDEN; i++)
    {
        batch[NN_BATCH_H1 + i] = h1[i];
        batch[NN_BATCH_H2 + i] = h2[i];
        batch[NN_BATCH_D1 + i] = d1[i];
        batch[NN_BATCH_D2 + i] = d2[i];
    }
    batch[NN_BATCH_D3 + 0] = dz.x;
    batch[NN_BATCH_D3 + 1] = dz.y;
    batch[NN_BATCH_D3 + 2] = dz.z;
    batch[NN_BATCH_D3 + 3] = 0.0;

    // feature gradients and Adam, bias correction with the visit count of this pixel
    float t = float(visit + 1);
    float correction1 = 1.0 - pow(ADAM_BETA1, t);
    float correction2 = 1.0 - pow(ADAM_BETA2, t);
    [loop]
    for (uint k = 0; k < NN_FEATURES; k++)
    {
        float g = 0.0;
        [loop]
        for (uint i = 0; i < NN_HIDDEN; i++)
        {
            g += w[NN_W1 + i * NN_IN + k] * d1[i];
        }

        uint f = pixelIndex * NN_FEATURES + k;
        float m = lerp(g, data.featureM[f], ADAM_BETA1);
        float v = lerp(g * g, data.featureV[f], ADAM_BETA2);
        data.featureM[f] = m;
        data.featureV[f] = v;
        data.features[f] -= Push.featureLR * (m / correction1) / (sqrt(v / correction2) + ADAM_EPSILON);
    }

    data.loss[tid] = isfinite(loss) ? loss : 0.0;
}
