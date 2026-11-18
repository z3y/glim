// Mirrors LightTreeNode in light_tree.rs

#pragma once

static const uint LIGHT_TREE_INVALID = 0xFFFFFFFF;
static const uint LIGHT_TREE_MAX_DEPTH = 128;

struct LightTreeNode
{
    float3 bmin;
    float energy;
    float3 bmax;
    float cosThetaO;
    float3 axis;
    float cosThetaE;
    uint left;
    uint right;
    uint parent;
    uint flags;
};

LightTreeNode LightTreeLoad(float4 *nodes, uint index)
{
    float4 a = nodes[index * 4 + 0];
    float4 b = nodes[index * 4 + 1];
    float4 c = nodes[index * 4 + 2];
    uint4 d = asuint(nodes[index * 4 + 3]);

    LightTreeNode n;
    n.bmin = a.xyz;
    n.energy = a.w;
    n.bmax = b.xyz;
    n.cosThetaO = b.w;
    n.axis = c.xyz;
    n.cosThetaE = c.w;
    n.left = d.x;
    n.right = d.y;
    n.parent = d.z;
    n.flags = d.w;
    return n;
}

float LightTreeCosSubClamped(float sinA, float cosA, float sinB, float cosB)
{
    if (cosA > cosB)
    {
        return 1.0;
    }
    return cosA * cosB + sinA * sinB;
}

float LightTreeSinSubClamped(float sinA, float cosA, float sinB, float cosB)
{
    if (cosA > cosB)
    {
        return 0.0;
    }
    return sinA * cosB - cosA * sinB;
}

float LightTreeImportance(LightTreeNode node, float3 p, float3 N, bool hasNormal)
{
    float3 c = 0.5 * (node.bmin + node.bmax);
    float3 diag = node.bmax - node.bmin;
    float radiusSq = 0.25 * dot(diag, diag);

    float3 toP = p - c;
    float distSq = dot(toP, toP);
    float d2 = max(max(distSq, radiusSq), 1e-8);
    float3 wi = toP * rsqrt(max(distSq, 1e-12));

    float cosW = dot(node.axis, wi);
    float sinW = sqrt(saturate(1.0 - cosW * cosW));

    float cosB = -1.0;
    if (distSq > radiusSq)
    {
        cosB = sqrt(saturate(1.0 - radiusSq / distSq));
    }
    float sinB = sqrt(saturate(1.0 - cosB * cosB));

    float cosO = node.cosThetaO;
    float sinO = sqrt(saturate(1.0 - cosO * cosO));

    float cosX = LightTreeCosSubClamped(sinW, cosW, sinO, cosO);
    float sinX = LightTreeSinSubClamped(sinW, cosW, sinO, cosO);
    float cosP = LightTreeCosSubClamped(sinX, cosX, sinB, cosB);

    if (cosP <= node.cosThetaE)
    {
        return 0.0;
    }

    float importance = node.energy * cosP / d2;

    if (hasNormal)
    {
        float cosI = dot(N, -wi);
        float sinI = sqrt(saturate(1.0 - cosI * cosI));
        importance *= max(LightTreeCosSubClamped(sinI, cosI, sinB, cosB), 0.0);
    }

    return max(importance, 0.0);
}

bool LightTreeSample(float4 *nodes, float3 p, float3 N, bool hasNormal, float u, out uint primitiveID, out float pmf)
{
    primitiveID = 0;
    pmf = 0.0;

    uint current = 0;
    float prob = 1.0;

    [loop]
    for (uint depth = 0; depth < LIGHT_TREE_MAX_DEPTH; depth++)
    {
        LightTreeNode node = LightTreeLoad(nodes, current);

        if (node.right == LIGHT_TREE_INVALID)
        {
            primitiveID = node.left;
            pmf = prob;
            return true;
        }

        LightTreeNode l = LightTreeLoad(nodes, node.left);
        LightTreeNode r = LightTreeLoad(nodes, node.right);

        float wl = LightTreeImportance(l, p, N, hasNormal);
        float wr = LightTreeImportance(r, p, N, hasNormal);
        float sum = wl + wr;

        if (!(sum > 0.0))
        {
            return false;
        }

        float pl = wl / sum;
        if (u < pl || wr <= 0.0)
        {
            u = min(u / pl, 0.99999994);
            prob *= pl;
            current = node.left;
        }
        else
        {
            float pr = wr / sum;
            u = clamp((u - pl) / pr, 0.0, 0.99999994);
            prob *= pr;
            current = node.right;
        }
    }

    return false;
}

float LightTreePmf(float4 *nodes, uint *primitiveToLeaf, uint primitiveID, float3 p, float3 N, bool hasNormal)
{
    uint current = primitiveToLeaf[primitiveID];
    if (current == LIGHT_TREE_INVALID)
    {
        return 0.0;
    }

    float pmf = 1.0;

    [loop]
    for (uint depth = 0; depth < LIGHT_TREE_MAX_DEPTH; depth++)
    {
        LightTreeNode node = LightTreeLoad(nodes, current);
        if (node.parent == LIGHT_TREE_INVALID)
        {
            return pmf;
        }

        LightTreeNode parent = LightTreeLoad(nodes, node.parent);
        uint siblingIndex = (parent.left == current) ? parent.right : parent.left;
        LightTreeNode sibling = LightTreeLoad(nodes, siblingIndex);

        float wc = LightTreeImportance(node, p, N, hasNormal);
        float ws = LightTreeImportance(sibling, p, N, hasNormal);

        if (!(wc > 0.0))
        {
            return 0.0;
        }

        pmf *= wc / (wc + ws);
        current = node.parent;
    }

    return 0.0;
}
