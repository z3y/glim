#ifndef GLIM_SG_LIGHTMAP_INCLUDED
#define GLIM_SG_LIGHTMAP_INCLUDED

// Glim Spherical Gaussian lightmaps
//
// Baked with Lightmap Mode: Spherical Gaussian. The lightmap texture (unity_Lightmap) is a 3x3 grid of tiles,
// laid out row by row from uv (0, 0), tile t is at column t % 3, row t / 3:
//   tile 0      the regular diffuse lightmap, unity_LightmapST points into this tile
//   tile 1 + k  RGB amplitude of lobe k
//
// 8 lobes on the corners of a cube in world space, all with the same sharpness:
//   radiance(dir) = sum_k amplitude_k * exp(sharpness * (dot(dir, axis_k) - 1))
// The lobes are a cosine weighted fit of the incoming light above the surface,
// the diffuse value has the same units as the regular lightmap (radiance of a white diffuse surface).
//
// Usage:
//   float2 lightmapUV = uv1 * unity_LightmapST.xy + unity_LightmapST.zw;
//   GlimSG sg = GlimSampleSG(unity_Lightmap, samplerunity_Lightmap, lightmapUV);
//   float3 diffuse = GlimSGDiffuse(sg, normalWS) * albedo;                       // instead of the lightmap sample, works with normal maps
//   float3 specular = GlimSGSpecular(sg, normalWS, viewDirWS, perceptualRoughness); // prefiltered radiance
//   specular *= specularColor * envBRDF.x + envBRDF.y;                               // split sum, same as a reflection probe sample
//
// Lightmap encoding: with High Quality (HDR) lightmap encoding the texture is linear and no decoding is needed.
// For other encodings define the decode before including this file, e.g. BIRP:
//   #define GLIM_SG_DECODE(c) DecodeLightmap(c)
//
// Must match shaders/includes/sg.slang and src/sg.rs

#ifndef GLIM_SG_DECODE
#define GLIM_SG_DECODE(c) ((c).rgb)
#endif

#define GLIM_SG_LOBE_COUNT 8
#define GLIM_SG_TILES 3
#define GLIM_SG_SHARPNESS 3.0

struct GlimSG
{
    float3 amplitude[GLIM_SG_LOBE_COUNT];
};

float3 GlimSGAxis(uint lobe)
{
    const float s = 0.57735026919; // 1 / sqrt(3)
    return float3((lobe & 1u) != 0u ? s : -s,
                  (lobe & 2u) != 0u ? s : -s,
                  (lobe & 4u) != 0u ? s : -s);
}

// lightmapUV is uv1 * unity_LightmapST.xy + unity_LightmapST.zw, inside the first tile
// clamped half a texel inside the tile, so bilinear filtering never reads a neighbouring tile
float2 GlimSGTileUV(float2 lightmapUV, uint tile, float2 texelSize)
{
    const float tileSize = 1.0 / GLIM_SG_TILES;
    float2 uv = clamp(lightmapUV, texelSize * 0.5, tileSize - texelSize * 0.5);
    return uv + float2(tile % GLIM_SG_TILES, tile / GLIM_SG_TILES) * tileSize;
}

float2 GlimSGTexelSize(Texture2D lightmap)
{
    float width, height;
    lightmap.GetDimensions(width, height);
    return 1.0 / float2(width, height);
}

// the regular diffuse lightmap from tile 0, the same as a regular lightmap sample without the bleeding at the tile edges
float3 GlimSampleSGDiffuseTile(Texture2D lightmap, SamplerState lightmapSampler, float2 lightmapUV)
{
    float2 uv = GlimSGTileUV(lightmapUV, 0, GlimSGTexelSize(lightmap));
    return GLIM_SG_DECODE(lightmap.SampleLevel(lightmapSampler, uv, 0));
}

GlimSG GlimSampleSG(Texture2D lightmap, SamplerState lightmapSampler, float2 lightmapUV)
{
    float2 texelSize = GlimSGTexelSize(lightmap);

    GlimSG sg;
    [unroll]
    for (uint lobe = 0; lobe < GLIM_SG_LOBE_COUNT; lobe++)
    {
        float2 uv = GlimSGTileUV(lightmapUV, lobe + 1, texelSize);
        sg.amplitude[lobe] = GLIM_SG_DECODE(lightmap.SampleLevel(lightmapSampler, uv, 0));
    }
    return sg;
}

// incoming radiance from a direction
float3 GlimSGEvaluate(GlimSG sg, float3 dir)
{
    float3 result = 0;
    [unroll]
    for (uint lobe = 0; lobe < GLIM_SG_LOBE_COUNT; lobe++)
    {
        result += sg.amplitude[lobe] * exp(GLIM_SG_SHARPNESS * (dot(dir, GlimSGAxis(lobe)) - 1.0));
    }
    return result;
}

// Stephen Hill's fitted approximation of integral(max(dot(n, w), 0) * SG(w) dw) for a unit amplitude lobe
// https://therealmjp.github.io/posts/sg-series-part-3-diffuse-lighting-from-an-sg-light-source/
float GlimSGIrradianceFitted(float muDotN, float lambda)
{
    const float c0 = 0.36;
    const float c1 = 1.0 / (4.0 * c0);

    float eml = exp(-lambda);
    float em2l = eml * eml;
    float rl = 1.0 / lambda;

    float scale = 1.0 + 2.0 * em2l - rl;
    float bias = (eml - em2l) * rl - em2l;

    float x = sqrt(1.0 - scale);
    float x0 = c0 * muDotN;
    float x1 = c1 * x;

    float n = x0 + x1;
    float y = abs(x0) <= x1 ? n * n / x : saturate(muDotN);

    const float PI = 3.14159265359;
    return (scale * y + bias) * (2.0 * PI / lambda);
}

// diffuse light for a normal, same units as the regular lightmap
float3 GlimSGDiffuse(GlimSG sg, float3 normal)
{
    const float INV_PI = 0.31830988618;
    float3 result = 0;
    [unroll]
    for (uint lobe = 0; lobe < GLIM_SG_LOBE_COUNT; lobe++)
    {
        float muDotN = dot(GlimSGAxis(lobe), normal);
        result += sg.amplitude[lobe] * GlimSGIrradianceFitted(muDotN, GLIM_SG_SHARPNESS);
    }
    return result * INV_PI;
}

// Incoming radiance prefiltered with the GGX lobe for the view direction, like a reflection probe sample.
// The GGX distribution is approximated by a SG around the normal and warped into the reflection direction,
// the result is the inner product of the lightmap lobes with that normalized SG.
// The lobes are broad, sharp reflections are not representable and very low roughness looks like roughness ~0.3
float3 GlimSGSpecular(GlimSG sg, float3 normal, float3 viewDir, float perceptualRoughness)
{
    float alpha = max(perceptualRoughness * perceptualRoughness, 0.02);
    float NoV = max(dot(normal, viewDir), 1e-4);
    float3 R = reflect(-viewDir, normal);

    float lambdaW = (2.0 / (alpha * alpha)) / (4.0 * NoV);
    float normalization = lambdaW / (1.0 - exp(-2.0 * lambdaW));

    float3 result = 0;
    [unroll]
    for (uint lobe = 0; lobe < GLIM_SG_LOBE_COUNT; lobe++)
    {
        float3 v = GLIM_SG_SHARPNESS * GlimSGAxis(lobe) + lambdaW * R;
        float lm = length(v);
        float inner = exp(lm - GLIM_SG_SHARPNESS - lambdaW) * (1.0 - exp(-2.0 * lm)) / lm;
        result += sg.amplitude[lobe] * inner;
    }
    return result * normalization;
}

#endif
