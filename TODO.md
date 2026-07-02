# Todo

## Features
- [x] UV Packing
  - [x] Scale offset
  - [ ] Per chart
  - [x] Padding
- [x] Spot lights
- [ ] Bake lightprobes with new shader
- [ ] Directional lightmaps
- [ ] Terrain Support
- [ ] Higher resolution alpha
- [ ] Sky light
- [ ] Light Cookies
- [ ] Area lights
- [ ] Light Probes Deringing
- [x] Bake reflection probes button with super sampling
- [ ] Shadowmask
- [ ] Subtractive
- [ ] Ambient Occlusion
- [ ] SH Lightmaps
- [ ] Adaptive Probe Volumes
- [ ] Light Volumes
- [ ] Probe occlusion
- [ ] Emissive multiplier
- [ ] Indirect multiplier
- [ ] Add support for CWBVH
- [ ] Global fix seams instead of per renderer
- [ ] Bake sky reflection probe

## Optimization
- [ ] Proper sync for bake loop
- [ ] Try to stop unity from slowing down the bake start for no reason
- [ ] Manually build the LightingData asset
- [ ] Adjust sample positions before baking
- [ ] Make seam detection faster
- [ ] Make emissive triangle detection check only emissive meshes
- [ ] Create visibility shader only once and reuse
- [ ] Memory optimizations (compress previous diffuse between bounces, destroy emission etc)
- [ ] Deduplicate light probe positions
- [x] Sample alpha in bake init shader as well to skip some rays

## Bugs
- [ ] Include OIDN dlls
- [x] Match point/spot light shadow radius
- [ ] Sync scene view fov
- [x] No licence yet
- [ ] Previous diffuse is flipped on Y
- [ ] handle not optimal swapchain
- [ ] Emissive triangles only detect opaque meshes
- [ ] Backface GI and Transparent flags are set for entire renderer instead of per submesh
- [ ] Some negatively scaled exported objects have flipped normals
- [ ] Preview emission doesnt have 1 bounce
- [ ] Can only bake one currently loaded scene
- [ ] Preview crashes when closing on linux
- [x] OpenGL unity is flipped xd
- [ ] Fix URP light falloff


# Complete
- [x] Log and progress callback
- [x] Alpha test
- [x] Conservative rasterization
- [x] Return codes for bake success, fail, cancel
- [x] Better panic handling
- [x] Seam stitching
- [x] Figure out why light probes are a bit darker
- [x] Blue noise
- [x] Double sided global illumination
- [x] Clamp max samples and bounces
- [x] Move test to another crate so gltf and image are not dependencies
- [x] Configurable nearest and linear sampler
- [x] Configurable probe samples and bounces
- [X] L2 SH
- [x] Export light probe positions and accumulate SH
- [x] Set all the globals in the unity meta pass
- [x] OIDN2 bindings and apply denoise

## Readme

- Supports only linear color space
