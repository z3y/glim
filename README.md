# Stilb Lightmap Baker

A GPU accelerated standalone lightmap baker for Unity, powered by Vulkan

## Notes

- Currently requires a GPU with `VK_KHR_ray_query` extension, however it will support any GPU with a software BVH in the future. Check GPU support here `https://vulkan.gpuinfo.org/listdevices.php`, most modern GPUs should work.
- While the lightmapper is fully working, it is still in early stages, theres room for improvement and it might lack some features

## Features

- Works on Windows and Linux (Nvidia and AMD GPUs)
- Fast hardware accelerated ray-tracing (can utilize RTX)
- Realtime Preview
- Open Image Denoise 2
- Seam stiching with a least squares solver
- UV Packing with hole filling
- Light Probe baking (L2 Spherical Harmonics)
- Light Volumes
- Physically correct
- Lightmap Groups
- Easy to use (aims to be mostly a drop in replacement)
- Small binary size (only ~1MB)
- Emissive materials, Directional, Spot and Point Lights
- Shadow radius
- Fully standalone, with Unity URP and Built-In pipeline support

## How to use

### Denoiser Setup

#### Windows

1. Download the Windows `.zip` from `https://github.com/RenderKit/oidn/releases`
2. Extract it anywhere on your computer (e.g. `C:\oidn`)
3. Set the `OpenImageDenoise_DIR` environment variable to that extracted folder:
   - Press **Start**, type **"environment variables"**, and open **"Edit environment variables for your account"**
   - Click **New...**
   - Name: `OpenImageDenoise_DIR`
   - Value: the path to the extracted folder (e.g. `C:\oidn`)
   - Click **OK** on all windows

#### Linux
- Fedora Linux: `sudo dnf install oidn`

### Baking

- Make sure to setup the denoiser first (otherwise denoising will be skipped)
- Setup the scene (mark objects as static, generate lightmap uvs, add lights with baked mode or emissive materials etc.)
- Menu Item `Stilb > Bake`
- Adjust settings on the created game object and press `Generate Lighting`

#### Lightmap Groups

 - By default one lightmap texture is baked containing the entire scene
 - To create multiple lightmaps add a Lightmap Group component to a game object
 - Create Lightmap Group asset and assign it to the component
 - All the child game objects will be packed into that group

## Stack

- Written in Rust, using the lightweight Ash vulkan crate, with minimal dependencies
- Shaders written in Slang

## Building

- Add the [slang](https://github.com/shader-slang/slang) compiler at PATH
- `cargo build --release`
