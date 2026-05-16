#pragma once

struct SHProbe
{
    float3 L0;
    uint pad0;

    float3 L1x;
    float positionX;

    float3 L1y;
    float positionY;

    float3 L1z;
    float positionZ;
};
