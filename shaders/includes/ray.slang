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
    bool frontFace;
}

// "A Fast and Robust Method for Avoiding Self-Intersection" (Carsten Wächter and Nikolaus Binder)
float3 RayOffset(float3 P, float3 Ng)
{
    const float origin = 1.0 / 32.0;
    const float floatScale = 1.0 / 65536.0;
    const float intScale = 256.0;

    // Use int3 for integer vector
    int3 of_i = (int3)float3(intScale * Ng.x, intScale * Ng.y, intScale * Ng.z);

    // Reinterpret bits and offset based on sign
    float3 p_i = float3(
        asfloat(asint(P.x) + ((P.x < 0.0) ? -of_i.x : of_i.x)),
        asfloat(asint(P.y) + ((P.y < 0.0) ? -of_i.y : of_i.y)),
        asfloat(asint(P.z) + ((P.z < 0.0) ? -of_i.z : of_i.z)));

    // Choose between integer-bit offset or small float-scale offset
    return float3(
        abs(P.x) < origin ? P.x + floatScale * Ng.x : p_i.x,
        abs(P.y) < origin ? P.y + floatScale * Ng.y : p_i.y,
        abs(P.z) < origin ? P.z + floatScale * Ng.z : p_i.z);
}
