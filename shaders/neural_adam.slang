#include "includes/neural.slang"

// Sums the partial gradients of neural_grad.slang and applies Adam to the shared MLP weights

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    float *weights;
    float *weightM;
    float *weightV;
    float *partials;

    uint chunkCount;
    uint batchSize;
    float learningRate;
    float step; // 1 based

    float weightDecay;
};

static const float ADAM_BETA1 = 0.9;
static const float ADAM_BETA2 = 0.99;
static const float ADAM_EPSILON = 1e-10;

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint weight = threadID.x;
    if (weight >= NN_WEIGHT_COUNT)
    {
        return;
    }

    float g = 0.0;
    for (uint chunk = 0; chunk < Push.chunkCount; chunk++)
    {
        g += Push.partials[chunk * NN_WEIGHT_COUNT + weight];
    }
    g /= float(Push.batchSize);

    if (!isfinite(g))
    {
        return;
    }

    float w = Push.weights[weight];
    g += Push.weightDecay * w;

    float m = lerp(g, Push.weightM[weight], ADAM_BETA1);
    float v = lerp(g * g, Push.weightV[weight], ADAM_BETA2);
    Push.weightM[weight] = m;
    Push.weightV[weight] = v;

    float correction1 = 1.0 - pow(ADAM_BETA1, Push.step);
    float correction2 = 1.0 - pow(ADAM_BETA2, Push.step);
    Push.weights[weight] = w - Push.learningRate * (m / correction1) / (sqrt(v / correction2) + ADAM_EPSILON);
}
