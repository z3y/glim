# Todo

- [ ] Higher resolution alpha
- [ ] Terrain Trees Support
- [ ] Light Cookies
- [x] Mixed Lights
  - [x] Baked Indirect
  - [ ] Shadowmask
  - [ ] Subtractive
- [ ] Ambient Occlusion
- [ ] Adaptive Probe Volumes
- [ ] Probe occlusion
- [ ] Emissive multiplier
- [ ] Bake sky reflection probe
- [ ] Per chart UV Packing
- [ ] Efficient LOD chart packing
- [ ] Meta fallback shader for mats without meta
- [ ] The manual denoiser setup is not ideal
- [ ] MIS for area lights
- [ ] There is no weight for MIS
- [ ] Disc lights

## Bugs
- [ ] Sync scene view fov
- [ ] Emissive triangles only detect opaque meshes
- [ ] Can only bake one currently loaded scene
- [ ] The slang extension complains about errors in IDE even though it all compiles
- [ ] Double sided global illumination doesnt work?

## Optimization
- [ ] Manually build the LightingData asset. This is one of the slowest things that happens before the bake starts becuse it has to start the built in baker (in an empty scene) for light probes tetrahedralization
- [ ] GPU Denoiser
