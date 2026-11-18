#define LIGHTPROBE_SH

#include "includes/core.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    SHProbeL2 *shProbes;
    uint probeCount;
};

const static float3 ProbeRayDirections[26] = {
    // Faces
    float3(0, 1, 0),
    float3(0, -1, 0),
    float3(1, 0, 0),
    float3(-1, 0, 0),
    float3(0, 0, 1),
    float3(0, 0, -1),

    // Edges
    normalize(float3(1, 1, 0)),
    normalize(float3(1, -1, 0)),
    normalize(float3(-1, 1, 0)),
    normalize(float3(-1, -1, 0)),

    normalize(float3(1, 0, 1)),
    normalize(float3(1, 0, -1)),
    normalize(float3(-1, 0, 1)),
    normalize(float3(-1, 0, -1)),

    normalize(float3(0, 1, 1)),
    normalize(float3(0, 1, -1)),
    normalize(float3(0, -1, 1)),
    normalize(float3(0, -1, -1)),

    // Corners
    normalize(float3(1, 1, 1)),
    normalize(float3(1, 1, -1)),
    normalize(float3(1, -1, 1)),
    normalize(float3(1, -1, -1)),
    normalize(float3(-1, 1, 1)),
    normalize(float3(-1, 1, -1)),
    normalize(float3(-1, -1, 1)),
    normalize(float3(-1, -1, -1)),
};

// runs once before baking light probes, moves probes stuck inside geometry and disables jitter near surfaces
[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint probeIndex = threadID.x;

    if (probeIndex >= Push.probeCount)
    {
        return;
    }

    SHProbeL2 *probes = Push.shProbes;

    SHProbeL2 probe = probes[probeIndex];

    Ray ray;
    ray.tMin = 0;
    ray.P = probe.position;

    Hit closestHit;
    bool found = false;
    float closestT = float ::maxValue;
    float halfTexel = probe.radius;
    float texelSize = halfTexel * 2.0;

    bool jitter = true;

    [loop]
    for (int i = 0; i < 26; i++)
    {
        ray.D = ProbeRayDirections[i];

        float c = abs(ray.D.x) + abs(ray.D.y) + abs(ray.D.z);
        ray.tMax = texelSize * c;

        Hit hit;
        if (TraceRayAlphaTested(ray, hit))
        {
            if (!hit.frontFace)
            {
                if (hit.t < closestT)
                {
                    closestT = hit.t;
                    closestHit = hit;
                    found = true;
                }
            }
            jitter = false;
        }
    }

    if (found)
    {
        Surface surface = Surface::Get(closestHit.primitiveID, closestHit.bary, true);
        probe.position = RayOffset(surface.P, surface.Ng);
        probe.radius = 0.0;
    }

    if (!jitter)
    {
        probe.radius = 0.0;
    }

    probes[probeIndex] = probe;
}
