#pragma once

#include "bindings.slang"

enum LightType : uint32_t
{
    Directional = 0,
    Point = 1,
    Spot = 2,
}

struct Light
{
    float3 position;
    LightType type;

    float3 direction;
    float range;

    float3 color;
    float ShadowRangeOrAngle;
};

void CreateOrthoBasis(float3 n, out float3 b1, out float3 b2)
{
    float s = (n.z >= 0.0) ? 1.0 : -1.0;
    float a = -1.0 / (s + n.z);
    float b = n.x * n.y * a;
    b1 = float3(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    b2 = float3(b, s + n.y * n.y * a, -n.y);
}

// L: Main light direction (pointing toward the light)
// angle: Spread angle in radians (e.g., radians(0.53) for the Sun)
// xi: Uniform random samples [0, 1]
float3 GetDirectionalLightAngle(float3 L, float angle, float2 xi)
{
    float cosThetaMax = cos(angle);

    // Uniformly sample the spherical cap (cone)
    float cosTheta = (1.0 - xi.x) + xi.x * cosThetaMax;
    float sinTheta = sqrt(max(0.0, 1.0 - cosTheta * cosTheta));
    float phi = xi.y * 6.28318530718; // 2 * PI

    // T and B form the tangent space around the light vector L
    float3 T, B;
    CreateOrthoBasis(normalize(L), T, B);

    // Compute the jittered sample direction (wi)
    float3 wi = T * (cos(phi) * sinTheta) +
                B * (sin(phi) * sinTheta) +
                L * cosTheta;

    return normalize(wi);
}

// thanks to error.mdl
// https://discord.com/channels/419351657743253524/443155141235834881/1429649996084416572
float GetUnityBuiltInAttenuation(float distanceSquare, float invRangeSq)
{
    float rangeFraction = distanceSquare * invRangeSq;
    float hyperbolaFalloff = (0.04 / (rangeFraction + 0.04));
    // starting at around 0.6407, formula linearly blends toward 0 at rangeFraction of 1
    float lerpToZeroFactor = saturate((rangeFraction - 0.6407) / (1.0 - 0.6407));
    float unityLightFalloff = lerp(hyperbolaFalloff, 0, lerpToZeroFactor);
    return unityLightFalloff;
}

float LightFalloff(float distanceSquare, float invRangeSq)
{
    if (LIGHT_FALLOFF_TYPE == 0)
    {
        // return 1.0 / distanceSquare;
        float factor = distanceSquare * invRangeSq;
        float smoothFactor = saturate(1.0 - factor * factor);
        return (smoothFactor * smoothFactor) / (distanceSquare + 1.0);
    }
    else
    {
        return GetUnityBuiltInAttenuation(distanceSquare, invRangeSq);
    }
}

float3 RandomPointOnOrientedDisk(float3 center, float radius, float3 positionWS, float2 xi)
{
    float3 centerToSurface = positionWS - center;
    float distToCenter = length(centerToSurface);
    float3 lightForward = centerToSurface / max(1e-5, distToCenter);

    float3 lightRight = normalize(cross(float3(0.0, 1.0, 0.0), lightForward));
    if (abs(lightForward.y) > 0.99)
    {
        lightRight = normalize(cross(float3(1.0, 0.0, 0.0), lightForward));
    }
    float3 lightUp = cross(lightForward, lightRight);

    float r = sqrt(xi.x);
    float phi = 2.0 * float.getPi() * xi.y;
    float2 diskPoint = float2(r * cos(phi), r * sin(phi));

    return center + (lightRight * diskPoint.x + lightUp * diskPoint.y) * radius;
}
