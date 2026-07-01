# Stilb Lightmap Baker

A GPU accelerated standalone lightmap baker for Unity, powered by Vulkan

## Notes

- Currently requires a GPU with `VK_KHR_ray_query` extension, however it will support any GPU with a software fallback in the future. Check GPU support here `https://vulkan.gpuinfo.org/listdevices.php`, most modern GPUs should work.
- While the lightmapper is fully working, it is still in early stages, theres room for improvement and it might lack some features

## Features

- Works on Windows and Linux
- Fast hardware accelerated ray-tracing (can utilize RTX)
- Realtime Preview
- Denoiser
- Seam stiching with a least squares solver
- Light Probe baking (L2 Spherical Harmonics)
- UV Packing
- Physically correct
- Lightmap Groups
- Easy to use (aims to be mostly a drop in replacement)
- Small binary size
- Emissive materials, Directional, Spot and Point Lights
- Fully standalone, with Unity URP and Built-In pipeline support

## How to use

1. todo

## Stack

- Written in Rust, using the lightweight Ash vulkan crate, with minimal dependencies
- Shaders written in Slang

## Building

- Add the [slang](https://github.com/shader-slang/slang) compiler to PATH
- `cargo build --release`
