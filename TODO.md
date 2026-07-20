# Todo

- [ ] Higher resolution alpha
- [ ] Terrain Support
- [ ] Light Cookies
- [ ] Light Probes Deringing
- [ ] Shadowmask
- [ ] Subtractive
- [ ] Ambient Occlusion
- [ ] SH Lightmaps
- [ ] Adaptive Probe Volumes
- [ ] Probe occlusion
- [ ] Emissive multiplier
- [ ] Indirect multiplier
- [ ] CWBVH (Implement https://github.com/jbikker/tinybvh BVH8_CWBVH with spatial splits)
- [ ] Bake sky reflection probe
- [ ] Per chart UV Packing
- [ ] Cancel bake button
- [ ] Efficient LOD chart packing
- [ ] Meta fallback shader for mats without meta
- [ ] Light Probes/Volumes are noisy with direct light from small emissives
  - [ ] Maybe denoise light probes or do MIS
- [ ] MIS for area lights
- [ ] The manual denoiser setup is not ideal

## Optimization
- [ ] Manually build the LightingData asset. This is one of the slowest things that happens before the bake starts becuse it has to start the built in baker (in an empty scene) for light probes tetrahedralization
- [ ] Make emissive triangle detection check only emissive meshes
- [ ] Submit multiple samples at once instead of waiting for faster ray tracing

## Bugs
- [ ] Sync scene view fov
- [ ] Emissive triangles only detect opaque meshes
- [ ] Backface GI and Transparent flags are set for entire renderer instead of per submesh
- [ ] Can only bake one currently loaded scene
- [ ] Preview crashes when closing the window on linux
- [ ] Preview window doesnt work on KDE Wayland (Fedora) in certain cases
- [ ] The slang extension complains about errors in IDE even though it all compiles
- [ ] Bake reflection probes button starts the built-in baker if the lighting is not baked which could cause confusion
- [ ] Double sided global illumination doesnt work?
