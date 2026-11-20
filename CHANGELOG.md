# [Glim](https://github.com/z3y/glim)

# v0.14.0

- The preview is now shown directly in the Scene view instead of the preview window
  - Drawn as a full screen quad before post processing, so the Scene view post processing (bloom, tonemapping, color grading) applies to it
  - Frames are now linear HDR (RGBA16F) instead of 8 bit sRGB
  - Follows the Scene view camera exactly: position, rotation, field of view and orthographic mode, moving the Scene view camera restarts the preview
  - Removed the preview window and its free fly controls (right mouse look, WASD)
  - Render resolution follows the Scene view size, the resolution scale option is still remembered
  - Stats, samples, bounces, resolution scale and a Stop button are in a Scene view overlay that is only shown while the preview is running
  - The Preview button on the Glim Lightmapper component now toggles the preview

# v0.13.0

- Implemented a light tree for emissive meshes
  - Replaces the Multiple Importance Sampling option, it's now called Light Tree and is on by default
  - Emissive triangles are picked based on their importance (distance, orientation and brightness) instead of uniformly at random, similar to pbrt and Cycles
  - Greatly reduces noise from emissive meshes, especially in scenes with a lot of emissive triangles or small and bright ones
  - Brightness of each emissive triangle is now estimated from multiple points on the triangle, instead of only checking if it's emissive or not
  - Affects lightmaps, light probes and light volumes
- Implemented solid angle sampling for emissive triangles
  - Reduces noise from large emissive triangles close to the surface
  - Falls back to area sampling for very small or very large triangles where it's numerically unreliable
- Fixed incorrect MIS weights for emissive meshes
  - BSDF pdf was a constant instead of using the cosine
- Improved Rectangle and Disc lights
  - Now sampled uniformly by solid angle instead of by area, which reduces noise when they are close to surfaces
  - Light cookies still work the same
- Implemented Double Sided Global Illumination
  - Uses the Double Sided Global Illumination option on materials, same as Unity
  - Back faces now get their own texels, so bounce rays hitting the back side of these meshes pick up lighting instead of being black
  - Works in the preview window too
- Implemented a shadow terminator fix
  - Based on "Hacking the Shadow Terminator" (Hanika 2021), adapted from Cycles
  - Shadow rays near the terminator start from a point lifted towards the smooth surface the vertex normals describe
  - Fixes blocky shadow artifacts on low poly meshes with smooth normals, this replaces the workaround removed in v0.12.0
  - Applies to direct lights, emissive meshes, shadowmasks and the preview window
  - On by default, can be disabled per renderer with `GlimRendererSettings` for meshes with bad normals (e.g. smooth normals across hard corners)
- Implemented light probe occlusion for shadowmask mode
  - Light probes now store occlusion for mixed lights, so dynamic objects get shadowmask shadows too
  - Written directly into the Lighting Data Asset
- Implemented Adaptive Probe Volumes support
  - Glim plugs into Unity's APV bake as the lighting backend, Unity still handles probe placement and serialization
  - APV probes get baked together with the rest of the light probes, including probe occlusion
  - Requires Unity 6 (render pipelines core 17)
- Shadowmasks now support soft shadows
  - Uses the light shadow radius or angle, same as direct lights
  - Sample count is raised to the direct light sample count when there are soft shadows, since shadowmasks are not denoised
- Seam fixer now skips seams with high contrast
  - High contrast is usually real lighting, for example a shadow edge lying on the seam, so it fades out the constraint instead of forcing both sides to match
  - Results are also clamped to the original range which removes the dark blotches it could create before
  - Uses the longer side of the seam to decide the sample count, so a side with higher texel density isn't undersampled
- Each indirect bounce after the first now uses half the samples of the previous one, with a minimum of 64
  - Later bounces carry less energy and are lower frequency, so this speeds up baking with more bounces without much visible difference
- Increased default indirect samples to 512
- Texels are now sorted in Morton order for better ray coherence
- Fixed some correlation between random samples of different baking stages
- Fixed Indirect Multiplier not affecting Combined SH lightmaps and the direction of Dominant Direction lightmaps
- UV packing is now ~2x faster
- Added an option to play a sound when the bake finishes or fails (thanks [Pesky12](https://github.com/pesky12))
- Benchmark timings are now logged in one line
- Implemented semi-transparent shadows for alpha blended (Transparent) materials
  - Shadows are no longer only fully blocked or not, light is reduced by the alpha of every transparent surface it passes through
  - Works with both Hardware and Software Ray-Tracing
  - Alpha tested (Cutout) materials still use the cutoff, grass and tree leaf materials are treated as cutout
  - Material color alpha is now multiplied with the alpha texture for both blended and cutout materials
  - Bounce rays pass through blended materials based on their alpha
  - Light probe occlusion and shadowmasks get partial occlusion too
- Implemented transmission for alpha blended materials
  - The covered part of the material can let light through tinted by its albedo, for colored shadows like tinted glass
  - Off by default, set per renderer with the new `GlimRendererSettings` component
- Added a `GlimRendererSettings` component for per renderer bake settings
  - Editor only, only affects the bake and doesn't change the materials
  - Double Sided GI override (Material, On, Off)
  - Emission multiplier, scales the baked emission of a renderer without using any extra memory, 0 removes it as a light source
  - Transmission
  - UV packing override (Group, ScaleOffset, SplitCharts)
  - Shadow terminator fix toggle
- ScaleOffset and SplitCharts UV packing can now be mixed in one lightmap group
  - Renderers that aren't Batching Static or have a GPU instanced material automatically use ScaleOffset in SplitCharts groups
- LODs now share the LOD0 lightmap
  - Only LOD0 is baked, lower LODs use the same lightmap and scale offset as LOD0, so their lightmap UVs need to match LOD0
  - LOD0 renderers with lower LODs always use ScaleOffset packing
  - Fixed lower LODs getting baked when assigned to a lightmap group with a Lightmap Group Selector
- The preview now opens in a Unity editor window instead of a separate GLFW window
  - Frames are read back to the CPU and uploaded to a texture, so it works with any editor graphics API (DirectX 11, DirectX 12, OpenGL Core and Vulkan)
  - Render resolution follows the window size, with a resolution scale option (100%, 75%, 50%, 25%) that is remembered
  - Samples and bounces moved from the Glim Lightmapper component to the preview window top bar, changing them restarts the preview
  - Frame throttling follows the editor Interaction Mode preference instead of its own setting
  - Same controls as before: hold right mouse to look, WASD to move, Q/E down/up, Shift faster, Ctrl slower, R restart, Esc to close
  - Removed the preview width, height, throttle, samples and bounces settings from the Glim Lightmapper component
- GLFW is no longer included in the plugin, it's only used for the standalone preview in tests or with the `glfw` cargo feature
- Fixed preview throttling alternating between long and short frames
- Preview should now work on macOS
- Albedo and emission atlases are now rendered conservatively
  - Fixes holes from thin or tiny triangles that didn't cover any texel center, without modifying material shaders
  - Each triangle is expanded on the CPU to cover every texel it touches (at most half a texel), with vertex attributes extrapolated so textures keep lining up
  - Drawn once before the regular jittered meta passes, so it only fills texels they miss
  - Built in parallel with Burst and shared between the albedo and emission atlases
  - Requires the Burst package, without it the conservative pass is skipped and an error is logged

# v0.12.5

- Fixed errors caused by meshes without UV0
  - This is required for sampling alpha, it will use UV1 instead
- Added log errors for meshes without both UV0 and UV1 or normals
- Clear button now deletes lightmap textures as well
- Refactored how .meta files are created for lightmap textures
  - This should fix issues caused by unity importing textures too early, for example when asset auto refresh is enabled
  - If the lightmaps didn't have proper import settings before, (for example looked incorrect on different platforms like android) press Clear and bake again
- Fixed incorrect directional lightmap encoding

# v0.12.4

- Handle some potential memory bugs with the new preview light updates

# v0.12.2

- Implemented sync for light properties
  - Changes to light settings affect the preview window now without having to relaunch it
  - Limited only to certain properties (example light cookie textures do not change)
- Preview window is now always on top
- Fixed artifacts on certain AMD GPUs
- Setting sample count to 0 now allows skipping certain baking stages
  - For example to completely disable emission set direct emission samples to 0


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
