# GPU Lightmapper

A GPU accelerated lightmapper for Unity, powered by Vulkan.
Easy to use, lightweight (only ~1MB) and fast.

## Features

- Nvidia, AMD, Intel GPUs
- Windows, Linux, [macOS](https://github.com/z3y/glim/issues/30)
- Hardware Ray-Tracing with [VK_KHR_ray_query](https://vulkan.gpuinfo.org/listdevicescoverage.php?extension=VK_KHR_ray_query)
- Software Ray-Tracing with [CWBVHs](https://research.nvidia.com/publication/2017-07_efficient-incoherent-ray-traversal-gpus-through-compressed-wide-bvhs)
- Realtime path-traced preview window
- Non-Directional, Dominant Direction and Monochromatic L1 SH Lightmaps
- Multisampling and Anti-Aliasing
- Unity BIRP and URP
- Directional, Point, Spot, Rectangle and Disc Lights
- RGB Light Cookies
- Emissive Materials with Multiple Importance Sampling
- UV Packing with hole filling and split charts mode
- Open Image Denoise 2.0
- Seam stitching with a least squares solver
- L2 SH Light Probe Baking
- [VRC Light Volumes](https://github.com/REDSIM/VRCLightVolumes)
- Mixed Lights Baked Indirect and Shadowmask mode
- Ambient Occlusion
- Lightmap Groups

## Support

- If you'd like to see it further improved, consider supporting on [Patreon](https://patreon.com/z3y)
- Alternatively you can consider buying some of my assets on [Booth](https://z3y.booth.pm/)

## How to use

### Lightmapper Setup

Make sure to also read [Updating](#updating) ⚠️

#### VPM Package

- Install with the [VRChat Creator Companion](https://z3y.github.io/vpm-package-listing/)

#### UPM Package

- Download the release https://github.com/z3y/glim/releases
- Unzip the folder, open the package manager in unity `Window > Package Manager`
- Click + add package from disk and select the `package.json` from that folder

### Denoiser Setup

#### Windows

1. Download the Windows `.zip` from [https://github.com/RenderKit/oidn/releases](https://github.com/RenderKit/oidn/releases)
2. Extract it anywhere on your computer (e.g. `C:\oidn`)
3. Set the `OpenImageDenoise_DIR` environment variable to that extracted folder (the folder containing bin, doc, include, lib):
   - Press **Start**, type **"environment variables"**, and open **"Edit environment variables for your account"**
   - Click **New...**
   - Name: `OpenImageDenoise_DIR`
   - Value: the path to the extracted folder (e.g. `C:\oidn`)
   - Click **OK** on all windows
4. Restart Unity and Unity Hub
5. After baking the log will show if OIDN loaded properly.

#### Linux

- Fedora Linux: `sudo dnf install oidn`
- Debian / Ubuntu: `sudo apt install libopenimagedenoise-dev`
- Arch Linux: `sudo pacman -S openimagedenoise`
- Alpine Linux: `sudo apk add openimagedenoise`
- NixOS: `nix-shell -p openimagedenoise`

#### macOS

- `brew install open-image-denoise`

### Baking
  
- Make sure to setup the denoiser first (otherwise denoising will be skipped)
- Setup the scene (mark GameObjects as static, generate lightmap uvs etc.)
- The lightmapper uses regular Unity light components, make sure to set them to baked
- Menu Item `Tools > Glim > Bake`
- Adjust settings on the created GameObject and press `Generate Lighting`

### Updating

- Make sure to close Unity first before updating to unload all currently loaded assemblies.
- Import the new package.

### Lightmap Groups

 - By default one lightmap texture is baked containing the entire scene
 - To create multiple lightmaps add a `Lightmap Group Selector` component to a game object
 - Right click, `Create > Lightmap Group (Glim)` asset and assign it to the selector component
 - All the child GameObjects will be packed into that group

### Additional Light Settings

For additional settings for baking you can add a `GlimAdditionalLightSettings` component to a `Light`

- Exposes the inner angle for spot lights and draws a gizmo for the Built-In render pipeline
- Adds an Area (Rectangle) Light option for a cookie texture

## Screenshots

![Lightmaps](/images/lightmaps.jpg)

### Preview Window

![Preview Window](/images/preview.jpg)

### Lightmap

![Lightmap](/images/Lightmap-0_comp_light.jpg)

### Directional Lightmap

![Directional Lightmap](/images/Lightmap-0_comp_dir.jpg)

## AI Disclosure

AI tools were used to assist with parts of the development process. 
AI Generated PRs are accepted, as long as they're reasonable to review.

## Building

- Add the [slang](https://github.com/shader-slang/slang) shader compiler at PATH
- `cargo build --release`
- Copy the compiled dll into the `/Editor` folder
- Alternatively you can run the `test_preview` or `test_bake` tests without Unity

## [Discord](https://discord.gg/bw46tKgRFT)
