#define TINYBVH_IMPLEMENTATION
#include "tiny_bvh.h"
#include <cstdint>
#include <cstdlib>
#include <cstring>

using namespace tinybvh;

extern "C"
{
    struct BVHHandle
    {
        void *nodesData;
        size_t nodesSizeBytes;
        uint32_t nodesCount;
        void *trianglesData;
        size_t trianglesSizeBytes;
        uint32_t trianglesCount;
    };

    BVHHandle *bvh_build(float *triangles, uint32_t triCount)
    {
        BVH8_CWBVH gpu_bvh;
        bvhvec4 *verts = reinterpret_cast<bvhvec4 *>(triangles);
        gpu_bvh.BuildHQ(verts, triCount);

        BVHHandle *h = new BVHHandle();

        h->nodesCount = gpu_bvh.usedBlocks;
        h->nodesSizeBytes = (size_t)gpu_bvh.usedBlocks * sizeof(bvhvec4);
        h->nodesData = malloc(h->nodesSizeBytes);
        memcpy(h->nodesData, gpu_bvh.bvh8Data, h->nodesSizeBytes);

        h->trianglesCount = gpu_bvh.idxCount;
        h->trianglesSizeBytes = (size_t)gpu_bvh.idxCount * 3 * sizeof(bvhvec4);
        h->trianglesData = malloc(h->trianglesSizeBytes);
        memcpy(h->trianglesData, gpu_bvh.bvh8Tris, h->trianglesSizeBytes);

        return h;
    }

    uint32_t bvh_get_nodes_size(BVHHandle *h)
    {
        return h ? (uint32_t)h->nodesSizeBytes : 0;
    }
    uint32_t bvh_get_nodes_count(BVHHandle *h)
    {
        return h ? h->nodesCount : 0;
    }
    uint32_t bvh_get_triangles_size(BVHHandle *h)
    {
        return h ? (uint32_t)h->trianglesSizeBytes : 0;
    }
    uint32_t bvh_get_triangles_count(BVHHandle *h)
    {
        return h ? h->trianglesCount : 0;
    }

    void bvh_copy_nodes(BVHHandle *h, void *dst)
    {
        if (h && dst)
            memcpy(dst, h->nodesData, h->nodesSizeBytes);
    }
    void bvh_copy_triangles(BVHHandle *h, void *dst)
    {
        if (h && dst)
            memcpy(dst, h->trianglesData, h->trianglesSizeBytes);
    }

    void bvh_free(BVHHandle *h)
    {
        if (!h)
            return;
        free(h->nodesData);
        free(h->trianglesData);
        delete h;
    }

} // extern "C"
