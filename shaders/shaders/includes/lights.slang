enum LightType : uint32_t
{
    Directional = 0,
    Point = 1,
    Spot = 2,
}

struct Light
{
    LightType type;
    float3 position;

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

float GetLightFalloff(float distanceSquare, float invRangeSq)
{
    float factor = distanceSquare * invRangeSq;
    float smoothFactor = saturate(1.0 - factor * factor);
    return (smoothFactor * smoothFactor) / (distanceSquare + 1.0);
}
