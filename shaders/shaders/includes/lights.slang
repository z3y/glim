enum LightType : uint
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
