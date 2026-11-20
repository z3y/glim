#include "includes/compaction.slang"
#include "includes/core.slang"
#include "includes/sobol_burley.slang"
#include "includes/neural_target.slang"

// Debug evaluation per pixel, at the surface normal:
//   eval[p * 16 + 0..3]   network diffuse (cosine lobe)
//   eval[p * 16 + 4..7]   network specular, SG lobe with the debug sharpness
//   eval[p * 16 + 8..11]  reference of that specular lobe, many traced rays
//   eval[p * 16 + 12..15] reference diffuse, many traced rays

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint2 *compaction;
    LightmapInfo *lightmapsInfo;

    NeuralData *data;
    uint referenceRays;
    float debugSharpnessT;

    uint pixelOffset;
    uint pixelCount;
};

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint pixelIndex = threadID.x + Push.pixelOffset;
    if (pixelIndex >= Push.pixelCount)
    {
        return;
    }

    NeuralData *data = Push.data;
    NeuralPixel pixel = data.pixels[pixelIndex];

    NNRng rng = NNRng(pixelIndex, 0xABCDu);
    NeuralSurface surface = NeuralCreateSurface(data, pixel, 0, 0.0);

    float *w = data.weights;
    float h1[NN_HIDDEN];
    float h2[NN_HIDDEN];
    float x[NN_IN];

    NNBuildInput(data.features, pixelIndex, surface.normalWS, NN_LOBE_COSINE, x);
    float3 diffuse = NNRadiance(NNForward(w, x, h1, h2));

    float t = Push.debugSharpnessT;
    NNBuildInput(data.features, pixelIndex, surface.normalWS, NNLobeCodeSG(t), x);
    float3 specular = NNRadiance(NNForward(w, x, h1, h2));

    float3 specularReference = 0.0;
    float3 diffuseReference = 0.0;
    if (Push.referenceRays > 0)
    {
        specularReference = NeuralTarget(data, surface, surface.normalWS, false, NNSharpness(t), Push.referenceRays, rng);
        diffuseReference = NeuralTarget(data, surface, surface.normalWS, true, 0.0, Push.referenceRays, rng);
    }

    float *eval = data.eval + pixelIndex * 16;
    eval[0] = diffuse.x;
    eval[1] = diffuse.y;
    eval[2] = diffuse.z;
    eval[3] = 1.0;
    eval[4] = specular.x;
    eval[5] = specular.y;
    eval[6] = specular.z;
    eval[7] = 1.0;
    eval[8] = specularReference.x;
    eval[9] = specularReference.y;
    eval[10] = specularReference.z;
    eval[11] = 1.0;
    eval[12] = diffuseReference.x;
    eval[13] = diffuseReference.y;
    eval[14] = diffuseReference.z;
    eval[15] = 1.0;
}
