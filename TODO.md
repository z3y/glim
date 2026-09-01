# Todo

- [ ] Higher resolution alpha
- [ ] Terrain Trees Support
- [ ] Light Cookies
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
- [ ] MIS for area lights
- [ ] The manual denoiser setup is not ideal
- [ ] There is no weight for MIS
- [ ] Disc lights

## Optimization
- [ ] Manually build the LightingData asset. This is one of the slowest things that happens before the bake starts becuse it has to start the built in baker (in an empty scene) for light probes tetrahedralization
- [ ] GPU Denoiser

## Bugs
- [ ] Sync scene view fov
- [ ] Emissive triangles only detect opaque meshes
- [ ] Can only bake one currently loaded scene
- [ ] The slang extension complains about errors in IDE even though it all compiles
- [ ] Bake reflection probes button starts the built-in baker if the lighting is not baked which could cause confusion
- [ ] Double sided global illumination doesnt work?
