#pragma once

[vk::constant_id(0)]
const uint HARDWARE_RT;

[vk::constant_id(1)]
const uint LIGHT_FALLOFF_TYPE;

[vk::constant_id(2)]
const uint JITTER_BARYCENTRICS;

[vk::constant_id(3)]
const uint EMISSIVE_TRIANGLES_COUNT;

// 1 = spectral products of colors (bounce albedo, tinted transparency), see spectral.slang
[vk::constant_id(4)]
const uint SPECTRAL_RENDERING;

[vk::constant_id(5)]
const uint LIGHTMAP_GROUP_COUNT;

[vk::constant_id(6)]
const uint LIGHTMAP_MODE;

[vk::constant_id(7)]
const uint COORDINATE_SPACE;

[vk::constant_id(8)]
const float SKYBOX_INTENSITY;

[vk::constant_id(9)]
const float INDIRECT_INTENSITY;

[vk::constant_id(10)]
const float LIGHTPROBE_DERINGING;

[vk::constant_id(11)]
const float EMISSIVE_MULTIPLIER;

[vk::constant_id(12)]
const float AO_INTENSITY;

[vk::constant_id(13)]
const float AO_RANGE;

[vk::constant_id(14)]
const uint BACKFACE_TEXEL_START;

// number of light spectra, 0 if there are none or spectral rendering is off
[vk::constant_id(15)]
const uint LIGHT_SPECTRA_COUNT;

[vk::constant_id(16)]
const uint GEOMETRIES_0_OFFSET;

[vk::constant_id(17)]
const uint GEOMETRIES_1_OFFSET;

[vk::constant_id(18)]
const uint GEOMETRIES_2_OFFSET;

[vk::constant_id(19)]
const uint GEOMETRIES_3_OFFSET;

enum LightmapMode : uint
{
    NonDirectional = 0,
    DominantDirection = 1,
    CombinedSH = 3,
};
