# Todo

## Priority
- [ ] Include OIDN dlls
- [ ] Fix directional lightmap import settings

## Features
- [ ] Directional lightmaps
- [ ] Higher resolution alpha
- [ ] Terrain Support
- [ ] Sky light
- [ ] Light Cookies
- [ ] Area lights
- [ ] Light Probes Deringing
- [ ] Shadowmask
- [ ] Subtractive
- [ ] Ambient Occlusion
- [ ] SH Lightmaps
- [ ] Adaptive Probe Volumes
- [ ] Probe occlusion
- [ ] Emissive multiplier
- [ ] Indirect multiplier
- [ ] Add support for CWBVH
- [ ] Bake sky reflection probe
- [ ] Per chart UV Packing
- [ ] Light Probes/Volumes are noisy with direct light from small emissives
  - [ ] Maybe denoise or do MIS

## Optimization
- [ ] Manually build the LightingData asset
- [ ] Make seam detection faster
- [ ] Make emissive triangle detection check only emissive meshes
- [ ] Memory optimizations (compress previous diffuse between bounces, destroy emission etc)

## Bugs
- [ ] Sync scene view fov
- [ ] Emissive triangles only detect opaque meshes
- [ ] Backface GI and Transparent flags are set for entire renderer instead of per submesh
- [ ] Can only bake one currently loaded scene
- [ ] Preview crashes when closing on linux
- [ ] Preview window doesnt work on KDE Wayland (Fedora)
- [ ] Bake reflection probes button starts the built-in baker if the lighting is not baked which could cause confusion
- [ ] Bakery is removing directional lightmaps
- [ ] Double sided global illumination doesnt work?
