#include "bake_bindings.slang"

static const float RAY_MAX = 10000.0;

struct Ray
{
    float3 P;
    float tMin;
    float3 D;
    float tMax;
}

struct Hit
{
    float2 bary;
    float t;
    uint primitiveID;
}

float4 EncodeVisibilityBuffer(float2 bary, uint primitiveID, bool isVisible, bool useSmoothP)
{
    uint encoded = 0;
    if (isVisible)
    {
        encoded |= 1 << 0;
    }
    if (useSmoothP)
    {
        encoded |= 1 << 1;
    }
    return float4(bary, asfloat(primitiveID), asfloat(encoded));
}

// Surface GetSurface<let T : uint>(RayQuery<T> query, RayDesc ray)
// {
//     return GetSurface(query.CommittedPrimitiveIndex(), query.CommittedTriangleBarycentrics());
// }

// hit is not always initialized
#pragma warning(disable: 41018)
bool TraceRay(Ray ray, out Hit hit)
{
    RayDesc rayDesc;
    rayDesc.Origin = ray.P;
    rayDesc.Direction = ray.D;
    rayDesc.TMin = ray.tMin;
    rayDesc.TMax = ray.tMax;

    RayQuery<RAY_FLAG_FORCE_OPAQUE> query;
    query.TraceRayInline(TopLevelAS, RAY_FLAG_FORCE_OPAQUE, 0xFF, rayDesc);

    while (query.Proceed())
    {
        // handle alpha testing
    }

    if (query.CommittedStatus() != COMMITTED_NOTHING)
    {
        hit.bary = query.CommittedTriangleBarycentrics();
        hit.primitiveID = query.CommittedPrimitiveIndex();
        hit.t = query.CommittedRayT();
        return true;
    }

    return false;
}
#pragma warning(default: 41018)

bool TraceShadowRay(Ray ray)
{
    RayDesc rayDesc;
    rayDesc.Origin = ray.P;
    rayDesc.Direction = ray.D;
    rayDesc.TMin = ray.tMin;
    rayDesc.TMax = ray.tMax;

    RayQuery<RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH | RAY_FLAG_FORCE_OPAQUE> query;
    query.TraceRayInline(TopLevelAS, RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH | RAY_FLAG_FORCE_OPAQUE, 0xFF, rayDesc);

    while (query.Proceed())
    {
        // handle alpha testing
    }

    if (query.CommittedStatus() != COMMITTED_NOTHING)
    {
        return true;
    }

    return false;
}
