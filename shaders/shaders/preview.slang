#include "includes/core.slang"
#include "includes/sobol_burley.slang"

// #define MIS

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint lightCount;
    uint maxSamples;

    uint sampleIndex;
    uint width;
    uint height;
    uint bounceCount;
};

float Luminance(float3 rgb)
{
    return dot(rgb, float3(0.2126, 0.7152, 0.0722));
}

float MISWeight(float pdf1, float pdf2)
{
    float p1 = pdf1 * pdf1;
    float p2 = pdf2 * pdf2;
    return p1 / (p1 + p2);
}

float3 ShadeTriangleLight(uint primitiveID, Surface surface, float2 xi, float3 throughput, float randomEmissiveTrianglePdf, out float pdf)
{
    float r0 = sqrt(xi.x);
    float r1 = xi.y;
    float2 bary = float2(r0 * (1.0f - r1), r0 * r1);

    Surface emissiveSurface = Surface::Get(primitiveID, bary);

    float3 lightColor = SampleEmission(emissiveSurface.uv, emissiveSurface.lightmapGroup).rgb / float.getPi();

    if (!any(lightColor > 0.0))
    {
        pdf = 0.0;
        return 0.0;
    }

    float triangleArea = CalculateTriangleArea(primitiveID);
    float pdfArea = randomEmissiveTrianglePdf / triangleArea;

    float3 L = emissiveSurface.P;
    float3 positionToLight = L - surface.P;
    L = normalize(positionToLight);
    float distanceSquare = dot(positionToLight, positionToLight);

    float cosTheta = saturate(dot(surface.N, L));

    float cosThetaLight = dot(-L, emissiveSurface.N);

    if (emissiveSurface.backfaceGI)
    {
        cosThetaLight = saturate(abs(cosThetaLight));
    }
    else
    {
        cosThetaLight = saturate(cosThetaLight);
    }

    if (cosThetaLight <= 0.0001f)
    {
        pdf = 0.0;
        return 0.0;
    }

    float solidAngleWeight = cosThetaLight / (distanceSquare * pdfArea);

    float3 color = cosTheta * solidAngleWeight * lightColor * throughput;

    Ray shadowRay;
    shadowRay.P = RayOffset(surface.P, surface.Ng);
    shadowRay.tMin = 0;
    shadowRay.tMax = sqrt(distanceSquare) - 0.001f;
    shadowRay.D = L;
    [branch] if (any(color > 0.0))
    {
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    pdf = pdfArea * distanceSquare / cosThetaLight;

    return color;
}

void ShadeLight(inout Spectrum spectrum, Light light, Surface surface, float2 xi, float3 throughput)
{
    Ray shadowRay;
    shadowRay.P = RayOffset(surface.P, surface.Ng);
    shadowRay.tMin = 0;

    float falloff;
    float3 L;
    if (light.type == LightType::Directional)
    {
        L = GetDirectionalLightAngle(light.direction, light.ShadowRangeOrAngle, xi);
        falloff = 1.0;

        shadowRay.tMax = RAY_MAX;
    }
    else
    {
        L = RandomPointOnSphere(light.position, light.ShadowRangeOrAngle, xi);

        float3 positionToLight = L - surface.P;
        L = normalize(positionToLight);

        float distanceSquare = dot(positionToLight, positionToLight);

        float rangeSq = light.range * light.range;

        falloff = LightFalloff(distanceSquare, 1.0 / rangeSq);

        shadowRay.tMax = sqrt(distanceSquare);
    }

    float cosTheta = saturate(dot(surface.N, L));

    shadowRay.D = L;

    float3 color = cosTheta * falloff * light.color * throughput;

    [branch] if (any(color > 0.0))
    {
        if (TraceShadowRay(shadowRay))
        {
            color = 0.0;
        }
    }

    SpectrumAdd(spectrum, color, L);
}

[shader("compute")]
    [numthreads(8, 8, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;
    uint2 resolution = uint2(Push.width, Push.height);

    if (coords.x >= resolution.x || coords.y >= resolution.y)
    {
        return;
    }

    Spectrum spectrum = SpectrumZero();
    float3 throughput = float3(1.0);

    float4 visibility = VisibilityBuffer[coords];

    uint encoded = asuint(visibility.a);

    if (encoded == 0)
    {
        return;
    }
    uint emissiveTriangleCount = EMISSIVE_TRIANGLES_COUNT;

    uint primitiveID = asuint(visibility.z);
    float2 bary = visibility.xy;
    bool useSmoothP = (encoded & (1 << 1)) != 0;

    Surface surface = Surface::Get(primitiveID, bary);

    // debug emissive triangles
    // for (uint i = 0; i < emissiveTriangleCount; i++)
    // {
    //     uint emissivePrimitiveID = EmissiveTriangles[i];
    //     if (primitiveID == emissivePrimitiveID)
    //     {
    //         LightmapDiffuse[coords] = 0.5;
    //         return;
    //     }
    // }
    // LightmapDiffuse[coords] = 0.0;
    // return;

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint bounceCount = Push.bounceCount;
    uint lightCount = Push.lightCount;

    // todo set constant
    // this is actually incorrect in cycles
    // uint sobolIndexMask = reversebits(NextPowerOfTwo(sampleCount - 1) - 1);
    // uint sobolIndexMask = NextPowerOfTwo(sampleCount - 1) - 1;
    uint sobolIndexMask = ~0u;

    // todo temp
    if (USE_CAMERA == 0)
    {
        if (Push.sampleIndex == 0)
        {
            visibility = AdjustSamplePositions(surface, visibility);
            VisibilityBuffer[coords] = visibility;
            encoded = asuint(visibility.a);

            if (encoded == 0)
            {
                return;
            }

            primitiveID = asuint(visibility.z);
            bary = visibility.xy;
            useSmoothP = (encoded & (1 << 1)) != 0;
        }
    }

    SamplingPattern pattern = SamplingPattern::BlueNoisePure;
    if (USE_CAMERA > 0)
    {
        pattern = SamplingPattern::BlueNoiseFirst;
    }
    uint pixelSeed = RNGPixelInit(coords, pattern);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);
    uint dimension = 0;

    if (USE_CAMERA > 0)
    {
        float3 emission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
        float3 albedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
        spectrum.color += emission;
        throughput = albedo;
    }

    if (useSmoothP)
    {
        surface.P = surface.smoothP;
    }

    // direct light
    float2 xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);
    float2 xiBounce = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

    float sampleWeight = 1.0;
    // todo use t as weight for backface tolerance
    // todo count direct light hits

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        ShadeLight(spectrum, Lights[lightIndex], surface, xiLight, throughput);
    }

    float randomEmissiveTrianglePdf = 1.0 / emissiveTriangleCount;

    Ray bounceRay;
    bounceRay.P = RayOffset(surface.P, surface.Ng);
    bounceRay.D = RandomDirectionInHemisphere(surface.N, xiBounce);
    bounceRay.tMin = 0;
    bounceRay.tMax = RAY_MAX;

    for (uint bounceIndex = 0; bounceIndex < bounceCount; bounceIndex++)
    {
        float bsdfPdf = 1.0 / float.getPi();

        float triangleLightPdf = 0.0;
        float3 triangleLight = 0.0;
        if (emissiveTriangleCount > 0)
        {
            float2 xiTriangle = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);
            uint emissivePrimitiveID = EmissiveTriangles[uint(xiTriangle.x * (float)emissiveTriangleCount)];
            triangleLight = ShadeTriangleLight(emissivePrimitiveID, surface, xiLight, throughput, randomEmissiveTrianglePdf, triangleLightPdf);
#ifdef MIS
            SpectrumAdd(spectrum, triangleLight * MISWeight(triangleLightPdf, bsdfPdf), 0);
#endif
        }

        bool hitEmissive = false;
        Hit bounceHit;
        if (TraceRay(bounceRay, bounceHit))
        {
            Surface previousSurface = surface;
            surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary);
            bool backfaceGI = surface.backfaceGI;

            if (!bounceHit.frontFace)
            {
                if (!backfaceGI)
                {
                    if (bounceIndex == 0)
                    {
                        sampleWeight = saturate(bounceHit.t);
                    }
                    break;
                }

                surface.N = -surface.N;
                surface.Ng = -surface.Ng;
            }

            float3 bounceAlbedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;

            xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);
            xiBounce = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

            float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;

            if (emissiveTriangleCount > 0)
            {
                if (any(bounceEmission > 0.0))
                {
                    hitEmissive = true;
#ifdef MIS
                    float triangleArea = CalculateTriangleArea(bounceHit.primitiveID);
                    float3 positionToLight = surface.P - previousSurface.P;
                    float distSq = dot(positionToLight, positionToLight);
                    float cosThetaLight = saturate(dot(-normalize(positionToLight), surface.N));
                    float hypotheticalLightPdf = (randomEmissiveTrianglePdf / triangleArea) * distSq / cosThetaLight;
                    SpectrumAdd(spectrum, bounceEmission * throughput * MISWeight(bsdfPdf, hypotheticalLightPdf), 0);
#else
                    SpectrumAdd(spectrum, bounceEmission * throughput, 0);
#endif
                }
            }

            throughput *= bounceAlbedo;

            for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
            {
                ShadeLight(spectrum, Lights[lightIndex], surface, xiLight, throughput);
            }

            bounceRay.P = RayOffset(surface.P, surface.Ng);
            bounceRay.D = RandomDirectionInHemisphere(surface.N, xiBounce);
        }
        else
        {
            break;
        }
    }

    AccumulateColor(LightmapDiffuse, coords, sampleIndex, float4(spectrum.color, sampleWeight));
}
