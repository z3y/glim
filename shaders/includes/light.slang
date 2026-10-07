enum LightType : uint
{
    Directional = 0,
    Point = 1,
    Spot = 2,
    Area = 3,
    Disc = 4,
};

struct Light
{
    float3 position;
    LightType type;

    float3 direction;
    float range;

    float3 color;
    float ShadowRangeOrAngle;

    float SpotInnerPercent;
    float SpotOuter;
    float2 AreaSizeOrDiscRadius;

    float3 up;
    uint mixed;

    float2 cookieSize;
    uint cookie;
    float pad2;
};

void CreateOrthoBasis(float3 n, out float3 b1, out float3 b2)
{
    float s = (n.z >= 0.0) ? 1.0 : -1.0;
    float a = -1.0 / (s + n.z);
    float b = n.x * n.y * a;
    b1 = float3(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    b2 = float3(b, s + n.y * n.y * a, -n.y);
}

float3 GetDirectionalLightAngle(float3 L, float angle, float2 xi)
{
    float cosThetaMax = cos(angle);

    float cosTheta = (1.0 - xi.x) + xi.x * cosThetaMax;
    float sinTheta = sqrt(max(0.0, 1.0 - cosTheta * cosTheta));
    float phi = xi.y * float.getPi() * 2.0;

    float3 T, B;
    CreateOrthoBasis(normalize(L), T, B);

    // Compute the jittered sample direction (wi)
    float3 wi = T * (cos(phi) * sinTheta) +
                B * (sin(phi) * sinTheta) +
                L * cosTheta;

    return normalize(wi);
}

float LegacyFalloff(float t)
{
    const float steepness = 25.0;
    const float windowStart = 0.64;

    if (t >= 1.0)
    {
        return 0.0;
    }

    float curve = 1.0 / (1.0 + steepness * t);

    if (t <= windowStart)
    {
        return curve;
    }

    float window = (1.0 - t) / (1.0 - windowStart);
    return curve * window;
}

float UnityBuiltInLightAttenuation(float distanceSquare, float invRangeSq)
{
    return LegacyFalloff(distanceSquare * invRangeSq);
}

float LightFalloff(float distanceSquare, float invRangeSq, uint falloffType)
{
    if (falloffType == 0)
    {
        float factor = distanceSquare * invRangeSq;
        float smoothFactor = saturate(1.0 - factor * factor);
        // return (smoothFactor * smoothFactor) / (distanceSquare + 1.0);
        return smoothFactor / max(distanceSquare, 0.0001);
    }
    else
    {
        return UnityBuiltInLightAttenuation(distanceSquare, invRangeSq);
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

float SpotLightAngleAttenuation(float3 spotForward, float3 l, float spotScale, float spotOffset)
{
    float cd = dot(-spotForward, l);
    float attenuation = saturate(cd * spotScale + spotOffset);
    return attenuation * attenuation;
}

void SpotLightScaleOffset(float outerAngle, float innerAnglePercent, out float spotScale, out float spotOffset)
{
    float innerAngle = outerAngle / 100 * innerAnglePercent;
    innerAngle = innerAngle / 360 * float.getPi();
    outerAngle = outerAngle / 360 * float.getPi();
    float cosOuter = cos(outerAngle);
    spotScale = 1.0 / max(cos(innerAngle) - cosOuter, 1e-4);
    spotOffset = -cosOuter * spotScale;
}

float2 OctahedralEncode(float3 n)
{
    n /= (abs(n.x) + abs(n.y) + abs(n.z));
    float2 p = n.xy;
    if (n.z < 0.0)
    {
        float2 s = float2(p.x >= 0.0 ? 1.0 : -1.0, p.y >= 0.0 ? 1.0 : -1.0);
        p = (1.0 - abs(p.yx)) * s;
    }
    return p * 0.5 + 0.5;
}

float4 SampleCookieOctahedral(float3 dir, uint index)
{
    return SampleCookie(OctahedralEncode(normalize(dir)), index);
}

float2 SampleDiskConcentric(float2 u)
{
    float2 o = 2.0 * u - 1.0;
    if (o.x == 0.0 && o.y == 0.0)
    {
        return 0.0;
    }

    float r, theta;
    if (abs(o.x) > abs(o.y))
    {
        r = o.x;
        theta = (float ::getPi() * 0.25) * (o.y / o.x);
    }
    else
    {
        r = o.y;
        theta = (float ::getPi() * 0.5) - (float ::getPi() * 0.25) * (o.x / o.y);
    }
    return r * float2(cos(theta), sin(theta));
}
