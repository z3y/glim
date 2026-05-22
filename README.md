# Todo

## Priority
- [ ] Alpha test
- [ ] Spot lights
- [ ] Spot light cookie with default unity cookie
- [ ] UV Packing with scale offset
- [ ] Adjust sample positions
- [ ] Directional lightmaps

## Other
- [ ] Include OIDN dlls
- [ ] Area lights
- [ ] Add support for CWBVH
- [ ] UV Packing per chart
- [ ] Deringing
- [ ] Make seam detection faster
- [ ] SH Lightmaps
- [ ] Light tree
- [ ] Proper sync for bake loop
- [ ] Probe occlusion
- [ ] Try to stop unity from slowing down the bake start for no reason
- [ ] Bake reflection probes with super sampling

## Easy
- [ ] Sync scene view fov
- [ ] Log callback
- [ ] Match shadow radius

# Complete
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
