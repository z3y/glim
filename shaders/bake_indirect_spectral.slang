#include "includes/core.slang"
#include "includes/sobol_burley.slang"
#include "includes/light_tree.slang"

// Indirect light with spectral rendering: a path per texel and sample with 4 hero wavelengths,
// the same estimator as the spectral preview, instead of the RGB bounce buffer of bake_indirect.
// The texel's direct light (lights, emission, sky) is baked by the direct passes, the path
// starts at the first bounce. Each vertex gets light sampling (lights, emissive triangles with
// MIS) and the bounce ray picks up emission and sky, up to bounceCount bounces.

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint compactedCount;
    uint sampleIndex;
    uint sampleCount;
    uint bounceCount;

    uint lightCount;
    uint pad0;

    float *lightmap;
    // irradiance times the texel's albedo, see SpectralAlbedoCorrection
    float *reflected;

    Light *lights;
    float4 *lightTree;
    uint *primitiveToLeaf;
};

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.compactedCount)
    {
        return;
    }

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.sampleCount;
    uint bounceCount = Push.bounceCount;
    float sampleScale = 1.0 / float(sampleCount);
    float invPi = 1.0 / float.getPi();

    float3 texelPosition;
    float3 texelNormal;
    uint3 indexing;
    ShadowTerminator texelTerminator; // the texel's own shadow rays belong to the direct passes
    CreateSample(index, sampleIndex, sampleCount, texelPosition, texelNormal, indexing, texelTerminator);

    SpectralSample s = SpectralSample::Create(SpectralU(hash_uint2(index, 0x85ebca6bu), sampleIndex));

    float3 albedo = 0.0;
    bool hasAlbedo = TexelAlbedo(index, albedo);
    float4 receiver = hasAlbedo ? Reflectance::Create(albedo).Eval(s.phase) : float4(0.0);

    // spectral irradiance at the texel, in lightmap units like the direct passes
    float4 radiance = 0.0;
    float4 throughput = 1.0;
    float3 firstDirection = texelNormal;

    float3 P = texelPosition;
    float3 N = texelNormal;

    for (uint k = 1; k <= bounceCount + 1; k++)
    {
        float2 xiBounce = SobolBurleySample2D(indexing.x, DIMENSION_INDIRECT_BOUNCE + k - 1, indexing.y, indexing.z);

        Ray ray;
        ray.P = P;
        ray.D = RandomDirectionInHemisphereCosine(N, xiBounce);
        ray.tMin = 0;
        ray.tMax = RAY_MAX;

        if (k == 1)
        {
            firstDirection = ray.D;
        }

        Hit hit;
        if (!TraceRay(ray, hit))
        {
            // the sky seen from the texel is direct light
            if (k > 1)
            {
                radiance += throughput * EvalEmissionSpectrum(SampleSkybox(ray.D), s.phase);
            }
            break;
        }

        Surface surface = Surface::Get(hit.primitiveID, hit.bary, hit.frontFace);

        if (k == 1)
        {
            float ao = hit.t < AO_RANGE ? 0.0 : 1.0;
            throughput *= lerp(1.0, ao, AO_INTENSITY);
        }

        if (!hit.frontFace && !surface.backfaceGI)
        {
            break;
        }

        // emission found by the bounce ray, MIS with the light sampling of the previous vertex.
        // The emission seen from the texel is direct light.
        if (k > 1 && surface.emissive)
        {
            float3 emission = SampleSurfaceEmission(surface);
            if (any(emission > 0.0))
            {
                float weight = 1.0;
                if (EMISSIVE_TRIANGLES_COUNT > 0)
                {
                    float selectionPmf = LightTreePmf(Push.lightTree, Push.primitiveToLeaf, hit.primitiveID, P, N, true);
                    if (selectionPmf > 0.0)
                    {
                        float lightPdf = selectionPmf * TriangleLightPdf(hit.primitiveID, P, N, surface.P, false);
                        float bsdfPdf = saturate(dot(N, ray.D)) * invPi;
                        weight = MISWeight(bsdfPdf, lightPdf);
                    }
                }
                radiance += throughput * weight * EvalEmissionSpectrum(emission, s.phase);
            }
        }

        // the last ray only looks for emission and sky
        if (k > bounceCount)
        {
            break;
        }

        float3 hitAlbedo = SampleAlbedo(surface.coords, surface.lightmapGroup).rgb;
        throughput *= Reflectance::Create(hitAlbedo).Eval(s.phase);
        if (!any(throughput > 0.0))
        {
            break;
        }

        P = RayOffset(surface.P, surface.Ng);
        N = surface.N;
        ShadowTerminator terminator = ShadowTerminator::Create(hit.primitiveID, hit.bary, surface.Ng, hit.frontFace, surface.shadowTerminator);

        // lights, including mixed ones, their indirect light is baked
        float2 xiLight = DecorrelateSample2D(xiBounce, 0x68e31da4u);
        for (uint lightIndex = 0; lightIndex < Push.lightCount; lightIndex++)
        {
            Light light = Push.lights[lightIndex];
            float3 L;
            float2 xi = DecorrelateSample2D(xiLight, lightIndex);
            float4 current = ShadeLightDirectSpectral(light, P, N, xi, s, throughput, L, terminator);
            radiance += current * saturate(dot(L, N));
        }

        // emissive triangles, MIS with the next bounce ray
        if (EMISSIVE_TRIANGLES_COUNT > 0)
        {
            float xiPick = DecorrelateSample2D(xiBounce, 0xb5297a4du).x;
            float2 xiTriangle = DecorrelateSample2D(xiBounce, 0x1b56c4e9u);

            uint emissivePrimitiveID;
            float selectionPmf;
            if (LightTreeSample(Push.lightTree, P, N, true, xiPick, emissivePrimitiveID, selectionPmf))
            {
                float3 L;
                float pdf = 0.0;
                float3 current = ShadeTriangleLight(emissivePrimitiveID, P, N, xiTriangle, 1.0, selectionPmf, pdf, L, false, terminator);
                float NoL = saturate(dot(L, N));
                current *= MISWeight(pdf, NoL * invPi);
                radiance += throughput * EvalEmissionSpectrum(current * NoL, s.phase);
            }
        }
    }

    // Negative channels (a narrow spectrum outside the sRGB gamut) are kept per sample,
    // clamping them here would bias the average towards less saturated colors.
    float3 irradiance = s.ToRGB(radiance) * INDIRECT_INTENSITY;
    float3 reflected = s.ToRGB(radiance * receiver) * INDIRECT_INTENSITY;
    if (any(!isfinite(irradiance)) || any(!isfinite(reflected)))
    {
        irradiance = 0.0;
        reflected = 0.0;
    }

    float *lightmap = Push.lightmap;

    lightmap[index * 3 + 0] += irradiance.x * sampleScale;
    lightmap[index * 3 + 1] += irradiance.y * sampleScale;
    lightmap[index * 3 + 2] += irradiance.z * sampleScale;

    Push.reflected[index * 3 + 0] += reflected.x * sampleScale;
    Push.reflected[index * 3 + 1] += reflected.y * sampleScale;
    Push.reflected[index * 3 + 2] += reflected.z * sampleScale;

    if (LIGHTMAP_MODE == LightmapMode::CombinedSH)
    {
        SHProbeL2 sh = SHProbeL2::zero();
        AccumulateL1LightmapIndirect(sh, max(irradiance, 0.0), firstDirection, texelNormal);

        float3 L0 = sh.sh[0].xyz * sampleScale;

        uint indexL0 = index + Push.compactedCount;
        lightmap[indexL0 * 3 + 0] += L0.x;
        lightmap[indexL0 * 3 + 1] += L0.y;
        lightmap[indexL0 * 3 + 2] += L0.z;

        float3 combinedSH = EncodeCombinedSHL1(sh);
        combinedSH *= sampleScale;

        uint indexL1 = indexL0 + Push.compactedCount;
        lightmap[indexL1 * 3 + 0] += combinedSH.x;
        lightmap[indexL1 * 3 + 1] += combinedSH.y;
        lightmap[indexL1 * 3 + 2] += combinedSH.z;
    }
    else if (LIGHTMAP_MODE == LightmapMode::DominantDirection)
    {
        float3 dominantDirection = firstDirection * Luminance(max(irradiance, 0.0) * sampleScale);

        uint indexDir = index + Push.compactedCount;
        lightmap[indexDir * 3 + 0] += dominantDirection.x;
        lightmap[indexDir * 3 + 1] += dominantDirection.y;
        lightmap[indexDir * 3 + 2] += dominantDirection.z;
    }
}
