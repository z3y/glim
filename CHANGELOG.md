# [Glim](https://github.com/z3y/glim)

# v0.12.0

- Implemented anti-aliasing
  - Barycentris jittered up to half a texel size on every sample
  - Multiple jitter attempts until the jittered bary falls inside the triangle, similar to Cycles
  - Allows lightmaps to capture more detail, for example shadows from thin triangles and reduces jagged lines
- Implemented multi-sampling
  - Barycentrics and primitive IDs are now generated on the CPU with higher precision and more control, instead of rasterizing triangles on a GPU
  - Gets written directly into a compacted buffer instead of compacting later
  - Triangles that would previously overlap with conservative rasterization now get baked on their own, and the result weighted by their coverage
  - This means that if a lightmap texel covers multiple triangles it can now be shaded more accurately because it accounts for lighting from multiple sides
  - Coverage Min Threshold setting (default 0.05)
    - Specifies how much a texel should cover a triangle to be considered for baking
    - Texels covered only by one triangle (for example on outer edges of a mesh) are always used, no matter the threshold
  - Coverage Max Samples setting (default 4)
    - Samples generated at the same lightmap texel are sorted by their coverage
    - This option discards all samples above this value
- Refactored how leaks from back faces are detected
  - Anti-aliasing means that a leak ray test would have to run on every sample which would be too slow
  - Implemented a workaround for that by disabling anti-aliasing for texels that can leak to keep high performance
  - Picks the closest back face now instead of the one it first hits
    - This fixes leaks properly in cases where texels can hit multiple backfaces
  - Added a toggle since even leaks can look decent now with anti-aliasing
    - It is still on by default however, because block compression creates a lot of artifacts in such high contrast areas
    - This would require something like an RGB9e5 32 bit Shared Exponent Float format to look decent
- Slightly improved precision of barycentrics
  - Previously uvs were premultiplied by resolution which could potentially make precision worse
  - Resolution is now efficeintly stored per vertex and used when needed (only 4 bits since its a power of 2)
- Refactored shadow mask baking
  - Shadowmasks are now anti-aliased as well
- Removed a min clamp for light radius, so it can now actually be 0 
  - This was just a workaround because there was no anti-liasing before
- Removed the previously used workaround for shadow terminator artifacts on low poly meshes
  - This caused issues with some other geometry, it will be replaced eventually with something better
- Refactored how texels are dilated
  - Instead of just checking which valid pixel is the closest it now also respects coverage
  - This should reduce a few more leaks
- Refactored how empty texels are filled to reduce block compression artifacts
- Fixed a missing bounce from direct mixed lights for light probes
- Improved the performance of the preview shader
- Improved lightmapper inspector

# v0.11.0

- New higher quality UV packing mode: Split Charts
  - Each mesh is split into multiple charts and allows individual UV islands to get packed separately and more efficiently
  - Generated UV Meshes are stored in the Lighting Data Asset file itself, which Unity applies at runtime
  - At build time these UVs get merged into the statically batched mesh, without taking up additional memory
  - Only works with static batching
- GlimLightmapGroupSelector now works on Unity Terrains
  - Allows overriding the terrain lightmap resolution

# v0.10.0

- Implemented full resolution alpha texture sampling
  - Previously the alpha resolution was limited by the lightmap group resolution
  - The meta pass does not contain alpha information so it comes from material properties
  - Gets alpha from textures by property names _MainTex and _BaseMap, along with their scale and offset
  - Applies _Cutoff and _Color.a values
  - Handles Repeat and Clamp texture modes
  - This can be further expanded to cover more shaders with common property names
- Fixed a compile error in unity 6000
- Added additional validation checks for meshes
  - Objects with RectTransform are now skipped
  - Non triangle MeshTopology (such as Quads) now log an error instead of crashing

# v0.9.2

- Software BVH creation is now 60% faster, ray tracing performance is the same
- Fixed artifacts with some renderers with shadow casting set to off
- Added more information to the preview window title
- Warning log for objects with scale in lightmap 0
- Fixed validation layers not logging inside Unity console when enabled

# v0.9.1

- Seam fixer now searches for seams between different meshes too globally
  - If the meshes are on the same lightmap group and both have Stitch Seams enabled on the renderer seams will be stitched
  - Scenes made with modular assets can benefit from this
- Refactored logs and added more useful information

# v0.9.0

- Implemented shadow mask baking
- Reverted the change for URP light falloff since it was incorrect
- GlimAdditionalLightSettings now draws a shadow radius gizmo too

# v0.8.0

- Implemented Mesh Renderer component Cast Shadows option
  - Set to Off to prevent a mesh from casting shadows
  - Any other value then Off casts shadows from both sides
  - Useful for having lights inside meshes
- Implemented Light component No Shadows option
   - Previously all lights would cast shadows, even if the Shadow Type was set to No Shadows
- Much less visible seams, especially with directional lightmaps
- Allow renderers to contribute to baked lighting without taking up lightmap space at runtime
  - Toggle `Discard` on a lightmap group to skip saving it
  - Useful for using light probes or light volumes at runtime instead of lightmaps
  - This feature still requires proper lightmap UVs for best results, as it relies on the Meta pass for albedo and emission
  - Check https://github.com/z3y/glim/issues/19 for more details
- New Editor GUI for the GlimLightmapGroupSelector component
  - `New` button creates a new GlimLightmapGroup scriptable object in the current scene folder
  - Inlines the editor for the GlimLightmapGroup directly on the selector component for easier edits

# v0.7.0

- Added Ambient Occlusion baking
 - AO intensity and range parameters
 - Affects indirect light only
- Implemented faster GPU denoising
  - Open Image Denoise can now use other devices like CUDA, SYCL, HIP, METAL instead of only CPU
  - Logs currently selected device when baking
  - Results should be visually identical
- Added disc light type
- Implemented light cookies for Directional, Spot, Point, Rectangle and Disc lights
  - Directional cookie size
  - Colored cookie textures (uses only RGB channels)
  - This behavior does not match BIRP lights yet since they only sample alpha
  - Another difference is the spot cone attenuation is always applied, unlike in built-in
  - This behavior matches URP (and is likely more desirable anyways)
- Added a new component `GlimAdditionalLightSettings`
  - For additional settings for baking you can attach it to a Unity Light
  - Exposes the inner angle for spot lights and draws a gizmo for the Built-In render pipeline
  - Adds a Rectangle and Disc Light option for a cookie texture
- Implemented skybox reflection probe
  - Previously it was missing and was just sampling a default black cubemap
- Calculate ambient probe (the single global light probe that exists by default) with Unity apis instead of baking it in the lightmapper
  - This makes it sligtly more consistent, less hacky, and scales with the skybox intensity slider properly
- The skybox cubemap now uses the resolution set in the lighting window environment tab
- Added global multiplier for emissive materials
- Respect the stitch seams option on unity renderer components
- Matched the legacy light falloff of BIRP lights exactly
- Adjusted the light attenuation for Inverse Square mode to match URP lights
  - This results in lights being slightly brighter up close
- Added more tooltips
