#ifndef GLIM_SPECTRAL_INCLUDED
#define GLIM_SPECTRAL_INCLUDED

// Glim Lightmapper, Spectral Albedo Offset
//
// Lightmaps store irradiance and shaders multiply them by the albedo per RGB channel. With
// spectral rendering some colors can't be reached that way, e.g. a cyan surface under an orange
// light reflects a little red, but its albedo has no red to multiply the lightmap with.
// Lightmaps baked with "Spectral Albedo Offset" on the Glim Lightmapper expect to be multiplied by
// GlimSpectralLightmapAlbedo(albedo) instead, which is never 0 in a channel of a colored albedo.
// Gray albedos are unchanged, so the same lightmap is correct for them in any shader.
//
// Only use it for the baked diffuse GI term:
//   float3 diffuseGI = lightmap * GlimSpectralLightmapAlbedo(albedo);
// albedo is the linear diffuse albedo (what the meta pass outputs).

// must match SPECTRAL_ALBEDO_OFFSET in glim shaders/includes/spectral.slang
#define GLIM_SPECTRAL_ALBEDO_OFFSET 0.1

float3 GlimSpectralLightmapAlbedo(float3 albedo)
{
    float saturation = max(albedo.r, max(albedo.g, albedo.b)) - min(albedo.r, min(albedo.g, albedo.b));
    return albedo + GLIM_SPECTRAL_ALBEDO_OFFSET * saturation;
}

#endif
