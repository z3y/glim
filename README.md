# Stilb Lightmap Baker

A GPU accelerated standalone lightmap baker for Unity, powered by Vulkan

## Notes

- Currently requires a GPU with `VK_KHR_ray_query` extension, however it will support any GPU with a software fallback in the future. Check GPU support here `https://vulkan.gpuinfo.org/listdevices.php`, most modern GPUs should work.
- While the lightmapper is working, it is still in early stages and might lack some features

## Features

- Works on Windows and Linux
- Hardware accelerated ray-tracing (can take advantage of RTX)
- Realtime preview
- Denoiser
- Seam stiching with a least squares solver
- Light Probe baking (L2 Spherical Harmonics)
- UV Packing
- Physically correct
- Emissive materials, Directional, Spot and Point Lights
- Unity URP and Built-In pipeline supported

## Stack

- Written in Rust, using the lightweight Ash vulkan crate, with minimal dependencies
- Shaders written in Slang
