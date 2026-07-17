# Todo

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
- [ ] CWBVH
- [ ] Bake sky reflection probe
- [ ] Per chart UV Packing
- [ ] Cancel bake button
- [ ] Meta fallback shader for mats without meta
- [ ] Light Probes/Volumes are noisy with direct light from small emissives
  - [ ] Maybe denoise light probes or do MIS
- [ ] MIS for area lights

## Optimization
- [ ] Manually build the LightingData asset
- [ ] Make seam detection faster
- [ ] Make emissive triangle detection check only emissive meshes
- [ ] Submit multiple samples at once instead of waiting for faster ray tracing

## Bugs
- [ ] Sync scene view fov
- [ ] Emissive triangles only detect opaque meshes
- [ ] Backface GI and Transparent flags are set for entire renderer instead of per submesh
- [ ] Can only bake one currently loaded scene
- [ ] Preview crashes when closing the window on linux
- [ ] Preview window doesnt work on KDE Wayland (Fedora) in certain cases
- [ ] Bake reflection probes button starts the built-in baker if the lighting is not baked which could cause confusion
- [ ] Denoising can still make the seams more visible
- [ ] Double sided global illumination doesnt work?
