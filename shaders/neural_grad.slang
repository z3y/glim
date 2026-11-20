#include "includes/neural.slang"

// Weight gradients as a split-K reduction over the batch: thread (weight, chunk) sums the
// outer product terms of one chunk of samples into partials[chunk][weight].

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    float *batch;
    float *partials;
    uint batchSize;
    uint chunkSize;
    uint chunkCount;
};

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint weight = threadID.x % NN_WEIGHT_COUNT;
    uint chunk = threadID.x / NN_WEIGHT_COUNT;
    if (chunk >= Push.chunkCount)
    {
        return;
    }

    // which delta and which input make up this weight's gradient
    uint deltaOffset;
    uint inputOffset;
    bool bias = false;
    if (weight < NN_B1)
    {
        deltaOffset = NN_BATCH_D1 + (weight - NN_W1) / NN_IN;
        inputOffset = NN_BATCH_X + (weight - NN_W1) % NN_IN;
    }
    else if (weight < NN_W2)
    {
        deltaOffset = NN_BATCH_D1 + (weight - NN_B1);
        inputOffset = 0;
        bias = true;
    }
    else if (weight < NN_B2)
    {
        deltaOffset = NN_BATCH_D2 + (weight - NN_W2) / NN_HIDDEN;
        inputOffset = NN_BATCH_H1 + (weight - NN_W2) % NN_HIDDEN;
    }
    else if (weight < NN_W3)
    {
        deltaOffset = NN_BATCH_D2 + (weight - NN_B2);
        inputOffset = 0;
        bias = true;
    }
    else if (weight < NN_B3)
    {
        deltaOffset = NN_BATCH_D3 + (weight - NN_W3) / NN_HIDDEN;
        inputOffset = NN_BATCH_H2 + (weight - NN_W3) % NN_HIDDEN;
    }
    else
    {
        deltaOffset = NN_BATCH_D3 + (weight - NN_B3);
        inputOffset = 0;
        bias = true;
    }

    uint start = chunk * Push.chunkSize;
    uint end = min(start + Push.chunkSize, Push.batchSize);

    float sum = 0.0;
    for (uint b = start; b < end; b++)
    {
        float *sample = Push.batch + b * NN_BATCH_STRIDE;
        float input = bias ? 1.0 : sample[inputOffset];
        sum += sample[deltaOffset] * input;
    }

    Push.partials[chunk * NN_WEIGHT_COUNT + weight] = sum;
}
