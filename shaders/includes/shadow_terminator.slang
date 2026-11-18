// SPDX-FileCopyrightText: 2011-2022 Blender Foundation
// SPDX-FileCopyrightText: 2026 z3y
// SPDX-License-Identifier: Apache-2.0
//
// Adapted code from Blender Foundation (Cycles, kernel/light/sample.h:
// shadow_ray_smooth_surface_offset and shadow_ray_offset)

#pragma once

#include "bindings.slang"

// Shadow terminator fix for smooth normals on low poly meshes, based on
// "Hacking the Shadow Terminator" (Hanika 2021). Shadow rays leave from a point lifted toward the
// curved surface the vertex normals describe, so they no longer clip the flat triangle near the
// terminator.
//
// Creating one only stores the shading point, the offset itself is only computed in Origin for
// light directions close to the terminator (Cycles: "10-20% of rays will be affected").
struct ShadowTerminator
{
    uint primitiveID;
    float2 bary;
    float3 Ng; // already flipped for back faces
    bool frontFace;
    bool enabled;

    static ShadowTerminator None()
    {
        ShadowTerminator t;
        t.primitiveID = 0;
        t.bary = 0.0;
        t.Ng = 0.0;
        t.frontFace = true;
        t.enabled = false;
        return t;
    }

    // primitiveID, bary and frontFace as passed to Surface::Get, Ng from that surface
    static ShadowTerminator Create(uint primitiveID, float2 bary, float3 Ng, bool frontFace)
    {
        ShadowTerminator t;
        t.primitiveID = primitiveID;
        t.bary = bary;
        t.Ng = Ng;
        t.frontFace = frontFace;
        t.enabled = true;
        return t;
    }

    // Origin of a shadow ray toward L. Only rays close to the terminator are lifted,
    // with a smooth transition at the cutoff (Cycles default object setting).
    // Call it only for rays that are actually traced.
    float3 Origin(float3 positionWS, float3 N, float3 L)
    {
        if (!enabled)
        {
            return positionWS;
        }

        const float OFFSET_CUTOFF = 0.1;

        float NL = abs(dot(N, L));
        float NgL = dot(Ng, L);

        float amount = (NL < OFFSET_CUTOFF) ? saturate(2.0 - (NgL + NL) / OFFSET_CUTOFF)
                                            : saturate(1.0 - NgL / OFFSET_CUTOFF);

        [branch]
        if (amount > 0.0)
        {
            positionWS += SmoothSurfaceOffset() * amount;
        }

        return positionWS;
    }

    float3 SmoothSurfaceOffset()
    {
        Vertex v[3] = TriangleVertices(primitiveID);

        float s = frontFace ? 1.0 : -1.0;

        float3 V0 = v[0].position;
        float3 V1 = v[1].position;
        float3 V2 = v[2].position;
        float3 N0 = v[0].DecodeNormal() * s;
        float3 N1 = v[1].DecodeNormal() * s;
        float3 N2 = v[2].DecodeNormal() * s;

        // weights of V0, V1, V2
        float u = 1.0 - bary.x - bary.y;
        float w1 = bary.x;
        float w2 = bary.y;

        float3 P = V0 * u + V1 * w1 + V2 * w2;
        float3 n = N0 * u + N1 * w1 + N2 * w2; // unnormalized like in Cycles

        // parabolic approximation
        float a = dot(N2 - N0, V0 - V2);
        float b = dot(N2 - N1, V1 - V2);
        float c = dot(N1 - N0, V1 - V0);
        float h = a * u * (u - 1.0) + (a + b + c) * u * w1 + b * w1 * (w1 - 1.0);

        if (dot(n, Ng) > 0.0)
        {
            // local linear envelope
            float h0 = max(max(dot(V1 - V0, N0), dot(V2 - V0, N0)), 0.0);
            float h1 = max(max(dot(V0 - V1, N1), dot(V2 - V1, N1)), 0.0);
            float h2 = max(max(dot(V0 - V2, N2), dot(V1 - V2, N2)), 0.0);
            h0 = max(dot(V0 - P, N0) + h0, 0.0);
            h1 = max(dot(V1 - P, N1) + h1, 0.0);
            h2 = max(dot(V2 - P, N2) + h2, 0.0);
            h = max(min(min(h0, h1), h2), h * 0.5);
        }
        else
        {
            float h0 = max(max(dot(V0 - V1, N0), dot(V0 - V2, N0)), 0.0);
            float h1 = max(max(dot(V1 - V0, N1), dot(V1 - V2, N1)), 0.0);
            float h2 = max(max(dot(V2 - V0, N2), dot(V2 - V1, N2)), 0.0);
            h0 = max(dot(P - V0, N0) + h0, 0.0);
            h1 = max(dot(P - V1, N1) + h1, 0.0);
            h2 = max(dot(P - V2, N2) + h2, 0.0);
            h = min(-min(min(h0, h1), h2), h * 0.5);
        }

        return n * h;
    }
};
