# GPU Lightmapper

A GPU accelerated standalone lightmap baker for Unity, powered by Vulkan

## Features

- Supports Windows and Linux (Nvidia and AMD GPUs)
- Fast hardware accelerated ray-tracing (can utilize RTX)
- Easy to use (aims to be mostly a drop-in replacement)
- Realtime Preview
- Seam stitching with a least squares solver
- UV Packing with hole filling
- Open Image Denoise 2.0
- Light Probe baking (L2 Spherical Harmonics)
- VRCLightVolumes baking
- Physically correct
- Lightmap Groups
- Directional Lightmaps
- Small binary size (only ~1MB)
- Emissive materials, Directional, Spot, and Point Lights
- Fully standalone, with Unity URP and Built-In pipeline support

## How to use

## Notes

- Currently requires a GPU with the `VK_KHR_ray_query` extension, however it will support any GPU with a software BVH in the future. Check GPU support here `https://vulkan.gpuinfo.org/listdevices.php`, most modern GPUs should work.
- While the lightmapper is fully working, it is still in early stages, theres room for improvement and it might lack some features
- If you'd like to see it further improved, consider supporting on [Patreon](https://patreon.com/z3y)

### Denoiser Setup

#### Windows

1. Download the Windows `.zip` from [https://github.com/RenderKit/oidn/releases](https://github.com/RenderKit/oidn/releases)
2. Extract it anywhere on your computer (e.g. `C:\oidn`)
3. Set the `OpenImageDenoise_DIR` environment variable to that extracted folder:
   - Press **Start**, type **"environment variables"**, and open **"Edit environment variables for your account"**
   - Click **New...**
   - Name: `OpenImageDenoise_DIR`
   - Value: the path to the extracted folder (e.g. `C:\oidn`)
   - Click **OK** on all windows
4. Restart Unity and Unity Hub

#### Linux
- Fedora Linux: `sudo dnf install oidn`

### Baking

- Make sure to setup the denoiser first (otherwise denoising will be skipped)
- Setup the scene (mark objects as static, generate lightmap uvs, add lights with baked mode or emissive materials etc.)
- Menu Item `Glim > Bake`
- Adjust settings on the created GameObject and press `Generate Lighting`

#### Lightmap Groups

 - By default one lightmap texture is baked containing the entire scene
 - To create multiple lightmaps add a `Lightmap Group Selector` component to a game object
 - Right click, `Create > Lightmap Group (Glim)` asset and assign it to the selector component
 - All the child GameObjects will be packed into that group

## [Discord](https://discord.gg/bw46tKgRFT)
 
## Stack

- Written in Rust, using the lightweight Ash Vulkan crate, with minimal dependencies
- Shaders written in Slang

## Building

- Add the [slang](https://github.com/shader-slang/slang) shader compiler at PATH
- `cargo build --release`
- Copy the compiled dll into the `/Editor` folder
- Alternatively you can run the `test_preview` or `test_bake` tests without Unity


## Screenshots

World Link: https://vrchat.com/home/world/wrld_6d94340a-cf41-42f9-97f9-d94667e5cba0

![Lightmaps](/images/lightmaps.jpg)

### Preview Window

![Preview Window](/images/preview.jpg)

### Lightmap

![Lightmap](/images/Lightmap-0_comp_light.jpg)

### Directional Lightmap

![Lightmap](/images/Lightmap-0_comp_dir.jpg)
