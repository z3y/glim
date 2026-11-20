#pragma once

// Spectral rendering based on
// "Spectral Rendering, Part 1: Spectra" by Christoph Peters
// https://momentsingraphics.de/SpectralRendering1Spectra.html
//
// Reflectances (albedo, transmission tint) are bounded MESE spectra. An sRGB color is mapped
// to 3 trigonometric moments ("Fourier sRGB") through a precomputed table, the moments are turned
// into Lagrange multipliers in closed form and the reflectance at phase phi in [-pi, 0] is
//   a(phi) = atan(L0 + 2 L1 cos(phi) + 2 L2 cos(2 phi)) / pi + 1/2
//
// RGB emitters (lights, emission, skybox, the irradiance carried between bounces) are
// scale * reflectance * D65. Lights can instead have a spectrum (sodium, fluorescent, ...),
// stored relative to D65. All emitters share the D65 part, so one wavelength sampling table stores
// (r, g, b) * D65 / pdf next to the phase. Its density mixes D65 with the light spectra of the scene
// so emission lines get sampled. 4 stratified hero wavelengths are used per estimate, results are
// accumulated as RGB.
//
// The lightmap is RGB and so is the bounce buffer, spectral products only happen where two
// colors are multiplied: irradiance with albedo at a bounce and light with tinted transparency.
// Products where one side is gray are exact in RGB and skip the spectral path, which also avoids
// adding color noise there. Direct light from lights with a spectrum is additionally kept per
// texel in SPECTRAL_BINS bins, so the first bounce and the albedo correction still see their lines.
//
// Tables are built in spectral.rs, the layout must match:
//   [0, SPECTRAL_SAMPLING_ENTRIES)    (r, g, b weight, phase)
//   [SPECTRAL_LUT_BASE, + RES^3)      moments (c0, c1, c2, 0), sRGB encoded coordinates, x = red
//   [SPECTRAL_BIN_WEIGHTS_BASE, + 16) rgb weight of each bin
//   [SPECTRAL_LIGHT_SPECTRA_BASE, ..) per light spectrum: value at each sampling entry
//                                     (4 per float4), then the mean of each bin

#include "constants.slang"
#include "util.slang"

[[vk::binding(9, 0)]]
StructuredBuffer<float4> SpectralTables;

static const uint SPECTRAL_SAMPLING_ENTRIES = 1024;
static const uint SPECTRAL_LUT_RESOLUTION = 32;
static const float SPECTRAL_DELTA = 0.002;
static const uint SPECTRAL_WAVELENGTHS = 4;
static const uint SPECTRAL_BINS = 16;
static const uint SPECTRAL_ENTRIES_PER_BIN = SPECTRAL_SAMPLING_ENTRIES / SPECTRAL_BINS;

static const uint SPECTRAL_LUT_BASE = SPECTRAL_SAMPLING_ENTRIES;
static const uint SPECTRAL_BIN_WEIGHTS_BASE = SPECTRAL_LUT_BASE + SPECTRAL_LUT_RESOLUTION * SPECTRAL_LUT_RESOLUTION * SPECTRAL_LUT_RESOLUTION;
static const uint SPECTRAL_LIGHT_SPECTRA_BASE = SPECTRAL_BIN_WEIGHTS_BASE + SPECTRAL_BINS;
static const uint SPECTRAL_LIGHT_SPECTRUM_STRIDE = SPECTRAL_SAMPLING_ENTRIES / 4 + SPECTRAL_BINS / 4;

// must match lights.rs
static const uint LIGHT_SPECTRUM_NONE = 0xFFFFFFFFu;

static const float3 SPECTRAL_LUMINANCE = float3(0.2126, 0.7152, 0.0722);

bool SpectralEnabled()
{
    return SPECTRAL_RENDERING != 0;
}

bool HasLightSpectrum(uint spectrum)
{
    return LIGHT_SPECTRA_COUNT > 0 && spectrum != LIGHT_SPECTRUM_NONE;
}

bool IsGray(float3 c)
{
    float hi = max(c.x, max(c.y, c.z));
    float lo = min(c.x, min(c.y, c.z));
    return (hi - lo) <= 1e-4 * hi;
}

// 4 stratified wavelengths, weights include the 1 / 4 of the estimator
struct SpectralSample
{
    float4 phase;
    float3 weight[SPECTRAL_WAVELENGTHS];
    // sampling table entries, to look up light spectra
    uint4 index;

    static SpectralSample Create(float u)
    {
        SpectralSample s = {};
        // the tables are a single dummy entry with spectral rendering off
        if (!SpectralEnabled())
        {
            return s;
        }

        [unroll]
        for (uint k = 0; k < SPECTRAL_WAVELENGTHS; k++)
        {
            float uk = (u + float(k)) / float(SPECTRAL_WAVELENGTHS);
            uint index = min(uint(uk * float(SPECTRAL_SAMPLING_ENTRIES)), SPECTRAL_SAMPLING_ENTRIES - 1);
            float4 entry = SpectralTables[index];
            s.weight[k] = entry.xyz * (1.0 / float(SPECTRAL_WAVELENGTHS));
            s.phase[k] = entry.w;
            s.index[k] = index;
        }
        return s;
    }

    // RGB of a spectrum given at the 4 wavelengths
    float3 ToRGB(float4 spectrum)
    {
        return weight[0] * spectrum.x + weight[1] * spectrum.y + weight[2] * spectrum.z + weight[3] * spectrum.w;
    }
};

// Low discrepancy over sample indices (golden ratio sequence), scrambled per seed
float SpectralU(uint seed, uint sampleIndex)
{
    float offset = float(hash_uint(seed) >> 8) * (1.0 / 16777216.0);
    return frac(offset + float(sampleIndex) * 0.6180339887);
}

// Hero wavelength u for estimates driven by a 2D light sample
float SpectralUFromSample(float2 xi, uint salt)
{
    return float(hash_uint3(asuint(xi.x), asuint(xi.y), salt) >> 8) * (1.0 / 16777216.0);
}

float SpectralEncodeSRGB(float x)
{
    return x <= 0.0031308 ? 12.92 * x : 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

// Fourier sRGB moments of a linear sRGB reflectance in [0, 1]
float3 RGBToMoments(float3 rgb)
{
    float3 encoded = float3(SpectralEncodeSRGB(saturate(rgb.x)), SpectralEncodeSRGB(saturate(rgb.y)), SpectralEncodeSRGB(saturate(rgb.z)));
    float3 p = encoded * float(SPECTRAL_LUT_RESOLUTION - 1);
    uint3 i = min(uint3(p), uint3(SPECTRAL_LUT_RESOLUTION - 2));
    float3 f = saturate(p - float3(i));

    const uint N = SPECTRAL_LUT_RESOLUTION;
    uint base = SPECTRAL_LUT_BASE + i.x + i.y * N + i.z * N * N;

    float3 c000 = SpectralTables[base].xyz;
    float3 c100 = SpectralTables[base + 1].xyz;
    float3 c010 = SpectralTables[base + N].xyz;
    float3 c110 = SpectralTables[base + N + 1].xyz;
    float3 c001 = SpectralTables[base + N * N].xyz;
    float3 c101 = SpectralTables[base + N * N + 1].xyz;
    float3 c011 = SpectralTables[base + N * N + N].xyz;
    float3 c111 = SpectralTables[base + N * N + N + 1].xyz;

    float3 c00 = lerp(c000, c100, f.x);
    float3 c10 = lerp(c010, c110, f.x);
    float3 c01 = lerp(c001, c101, f.x);
    float3 c11 = lerp(c011, c111, f.x);
    return lerp(lerp(c00, c10, f.y), lerp(c01, c11, f.y), f.z);
}

// complex numbers as float2
float2 CMul(float2 a, float2 b)
{
    return float2(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

float2 CConj(float2 a)
{
    return float2(a.x, -a.y);
}

// Closed form bounded MESE (Peters et al. 2019) for 3 real trigonometric moments.
// Must match moments_to_lagrange in spectral.rs.
float3 MomentsToLagrange(float3 c)
{
    const float PI = float.getPi();

    // exponential moments, power series of exp(i pi (C(z) - 1/2)), C(z) = c0 + 2 c1 z + 2 c2 z^2
    float phase = PI * (c.x - 0.5);
    float2 f0 = float2(cos(phase), sin(phase));
    float2 iPi = float2(0.0, PI);
    float2 f1 = CMul(f0, iPi) * (2.0 * c.y);
    float2 f2 = CMul(f1 * c.y + f0 * (2.0 * c.z), iPi);

    // Toeplitz moments of the density
    float g0 = f0.x;
    float2 g1 = f1 * 0.5;
    float2 g2 = f2 * 0.5;

    // Levinson recursion, T q = e0 * E, T_jk = gamma_(j-k), q0 = 1
    float e = g0;
    float2 k1 = -g1 / e;
    float2 a1 = k1;
    e *= 1.0 - dot(k1, k1);
    float2 delta = g2 + CMul(g1, a1);
    float2 k2 = -delta / e;
    float2 q1 = a1 + CMul(k2, CConj(a1));
    float2 q2 = k2;

    // P = F * Q truncated to degree 2
    float2 p0 = f0;
    float2 p1 = f1 + CMul(f0, q1);
    float2 p2 = f2 + CMul(f1, q1) + CMul(f0, q2);

    // s_l = sum_{j - k = l} p_j conj(q_k), q0 = 1
    float2 s0 = p0 + CMul(p1, CConj(q1)) + CMul(p2, CConj(q2));
    float2 s1 = p1 + CMul(p2, CConj(q1));
    float2 s2 = p2;
    float2 sm1 = CMul(p0, CConj(q1)) + CMul(p1, CConj(q2));
    float2 sm2 = CMul(p0, CConj(q2));

    // lambda_l = Re((s_l - conj(s_-l)) / (2 i)) / Re(s_0) = Im(s_l - conj(s_-l)) / (2 Re(s_0))
    float scale = 0.5 / s0.x;
    float3 lagrange = float3(
        (s0.y + s0.y) * scale,
        (s1.y + sm1.y) * scale,
        (s2.y + sm2.y) * scale);

    if (any(!isfinite(lagrange)))
    {
        // degenerate moments, fall back to the constant spectrum with the same mean
        lagrange = float3(tan(PI * (clamp(c.x, 0.001, 0.999) - 0.5)), 0.0, 0.0);
    }
    return lagrange;
}

// A reflectance spectrum in [0, 1]
struct Reflectance
{
    float3 lagrange;

    static Reflectance Create(float3 rgb)
    {
        Reflectance r = {};
        r.lagrange = MomentsToLagrange(RGBToMoments(rgb));
        return r;
    }

    float4 Eval(float4 phase)
    {
        float4 c1 = cos(phase);
        float4 c2 = 2.0 * c1 * c1 - 1.0; // cos(2 phi)
        float4 x = lagrange.x + 2.0 * lagrange.y * c1 + 2.0 * lagrange.z * c2;
        float4 a = atan(x) * (1.0 / float.getPi()) + 0.5;
        // the table is solved for DELTA + (1 - 2 DELTA) * rgb
        return saturate((a - SPECTRAL_DELTA) * (1.0 / (1.0 - 2.0 * SPECTRAL_DELTA)));
    }
};

// Unbounded RGB emitter, scale * reflectance(rgb / scale) * D65 (D65 lives in the sampling weights).
// Scaling to a maximum of 0.5 keeps the spectrum smooth and away from the [0, 1] bounds.
float4 EvalEmissionSpectrum(float3 rgb, float4 phase)
{
    rgb = max(rgb, 0.0);
    float scale = 2.0 * max(rgb.x, max(rgb.y, rgb.z));
    if (!(scale > 0.0) || !isfinite(scale))
    {
        return 0.0;
    }
    Reflectance r = Reflectance::Create(rgb / scale);
    return scale * r.Eval(phase);
}

// Light spectrum (luminance 1) at a sampling table entry
float LightSpectrumAt(uint spectrum, uint entry)
{
    float4 values = SpectralTables[SPECTRAL_LIGHT_SPECTRA_BASE + spectrum * SPECTRAL_LIGHT_SPECTRUM_STRIDE + entry / 4];
    return values[entry % 4];
}

// Mean of the light spectrum over bins 4 * k to 4 * k + 3
float4 LightSpectrumBinMeans(uint spectrum, uint k)
{
    return SpectralTables[SPECTRAL_LIGHT_SPECTRA_BASE + spectrum * SPECTRAL_LIGHT_SPECTRUM_STRIDE + SPECTRAL_SAMPLING_ENTRIES / 4 + k];
}

// An emitter at the wavelengths of s. The color of a light with a spectrum has the spectrum's hue
// with the light's brightness as luminance (see app_add_light), its spectrum has luminance 1.
float4 EvalEmitter(float3 rgb, uint spectrum, SpectralSample s)
{
    if (!HasLightSpectrum(spectrum))
    {
        return EvalEmissionSpectrum(rgb, s.phase);
    }

    float4 values = float4(
        LightSpectrumAt(spectrum, s.index.x),
        LightSpectrumAt(spectrum, s.index.y),
        LightSpectrumAt(spectrum, s.index.z),
        LightSpectrumAt(spectrum, s.index.w));
    return dot(rgb, SPECTRAL_LUMINANCE) * values;
}

// Irradiance from lights with a spectrum, binned over ranges of the sampling table.
// Stored per texel next to the RGB lightmap, see bake_direct_light.
struct SpectralBins
{
    float4 bins[SPECTRAL_BINS / 4];

    static SpectralBins Zero()
    {
        SpectralBins b = {};
        return b;
    }

    static SpectralBins Load(float *buffer, uint texel)
    {
        SpectralBins b = {};
        if (LIGHT_SPECTRA_COUNT == 0)
        {
            return b;
        }

        [unroll]
        for (uint i = 0; i < SPECTRAL_BINS; i++)
        {
            b.bins[i / 4][i % 4] = buffer[texel * SPECTRAL_BINS + i];
        }
        return b;
    }

    void Add(float *buffer, uint texel)
    {
        [unroll]
        for (uint i = 0; i < SPECTRAL_BINS; i++)
        {
            buffer[texel * SPECTRAL_BINS + i] += bins[i / 4][i % 4];
        }
    }

    // light with a spectrum and the given luminance
    [mutating]
    void AddLight(uint spectrum, float luminance)
    {
        [unroll]
        for (uint k = 0; k < SPECTRAL_BINS / 4; k++)
        {
            bins[k] += luminance * LightSpectrumBinMeans(spectrum, k);
        }
    }

    bool IsZero()
    {
        float4 sum = 0.0;
        [unroll]
        for (uint k = 0; k < SPECTRAL_BINS / 4; k++)
        {
            sum += bins[k];
        }
        return all(sum <= 0.0);
    }

    float At(uint entry)
    {
        uint bin = entry / SPECTRAL_ENTRIES_PER_BIN;
        return bins[bin / 4][bin % 4];
    }

    float4 At(uint4 entries)
    {
        return float4(At(entries.x), At(entries.y), At(entries.z), At(entries.w));
    }

    float3 ToRGB()
    {
        float3 rgb = 0.0;
        [unroll]
        for (uint i = 0; i < SPECTRAL_BINS; i++)
        {
            rgb += bins[i / 4][i % 4] * SpectralTables[SPECTRAL_BIN_WEIGHTS_BASE + i].xyz;
        }
        return rgb;
    }
};

// Irradiance spectrum at the wavelengths of s: the binned light spectra plus the rest
// (RGB lights, emission, sky, bounces) upsampled from RGB.
float4 EvalIrradiance(float3 irradiance, SpectralBins bins, SpectralSample s)
{
    float3 remainder = max(irradiance - bins.ToRGB(), 0.0);
    return EvalEmissionSpectrum(remainder, s.phase) + bins.At(s.index);
}

// Spectral product of an emitter and an RGB reflectance, returned as RGB.
// Exact RGB product when spectral rendering is off or either factor is gray.
float3 SpectralMultiply(float3 emitter, float3 reflectance, float u, uint spectrum = LIGHT_SPECTRUM_NONE)
{
    bool grayEmitter = !HasLightSpectrum(spectrum) && IsGray(emitter);
    if (!SpectralEnabled() || grayEmitter || IsGray(reflectance))
    {
        return emitter * reflectance;
    }

    SpectralSample s = SpectralSample::Create(u);
    float4 e = EvalEmitter(emitter, spectrum, s);
    float4 a = Reflectance::Create(reflectance).Eval(s.phase);
    return s.ToRGB(e * a);
}

// Same for irradiance that has binned light spectra in it
float3 SpectralMultiply(float3 irradiance, SpectralBins bins, float3 reflectance, float u)
{
    if (bins.IsZero())
    {
        return SpectralMultiply(irradiance, reflectance, u);
    }
    if (!SpectralEnabled() || IsGray(reflectance))
    {
        return irradiance * reflectance;
    }

    SpectralSample s = SpectralSample::Create(u);
    float4 e = EvalIrradiance(irradiance, bins, s);
    float4 a = Reflectance::Create(reflectance).Eval(s.phase);
    return s.ToRGB(e * a);
}

// Lightmaps store irradiance and the albedo is multiplied in at runtime, per RGB channel.
// Returns per channel factors f so that albedo * (irradiance * f) equals the spectral product
// of the irradiance and albedo spectra, which is what the spectral preview shows.
// Deterministic quadrature over every 4th entry of the sampling table, no color noise.
float3 SpectralAlbedoCorrection(float3 irradiance, SpectralBins bins, float3 albedo)
{
    bool binned = !bins.IsZero();
    if (!SpectralEnabled() || (!binned && IsGray(irradiance)) || IsGray(albedo) || !any(irradiance > 0.0))
    {
        return 1.0;
    }

    float3 remainder = max(irradiance - bins.ToRGB(), 0.0);
    float scale = 2.0 * max(remainder.x, max(remainder.y, remainder.z));
    Reflectance e = Reflectance::Create(scale > 0.0 ? remainder / scale : float3(0.0));
    Reflectance a = Reflectance::Create(albedo);

    float3 product = 0.0;
    float3 reconstructed = 0.0;

    const uint STRIDE = 4;
    [loop]
    for (uint i = STRIDE / 2; i < SPECTRAL_SAMPLING_ENTRIES; i += STRIDE * 4)
    {
        uint4 entries = uint4(i, i + STRIDE, i + STRIDE * 2, i + STRIDE * 3);
        float4 s0 = SpectralTables[entries.x];
        float4 s1 = SpectralTables[entries.y];
        float4 s2 = SpectralTables[entries.z];
        float4 s3 = SpectralTables[entries.w];
        float4 phase = float4(s0.w, s1.w, s2.w, s3.w);

        float4 ev = (scale > 0.0 ? scale * e.Eval(phase) : float4(0.0)) + bins.At(entries);
        float4 ea = ev * a.Eval(phase);

        reconstructed += s0.xyz * ev.x + s1.xyz * ev.y + s2.xyz * ev.z + s3.xyz * ev.w;
        product += s0.xyz * ea.x + s1.xyz * ea.y + s2.xyz * ea.z + s3.xyz * ea.w;
    }

    // dark albedo channels contribute little at runtime anyway, don't blow the irradiance up for them
    const float MIN_ALBEDO = 0.02;
    const float MAX_FACTOR = 4.0;
    float threshold = 1e-4 * max(reconstructed.x, max(reconstructed.y, reconstructed.z));

    float3 factor = 1.0;
    [unroll]
    for (uint c = 0; c < 3; c++)
    {
        if (reconstructed[c] > threshold)
        {
            factor[c] = max(product[c] / (reconstructed[c] * max(albedo[c], MIN_ALBEDO)), 0.0);
        }
    }

    // A channel the albedo zeroes out can't be restored by the runtime multiply, the hue stays off
    // there (e.g. no red for a cyan albedo). At least match the brightness of the spectral result.
    float targetLuminance = dot(product * (irradiance / max(reconstructed, threshold)), SPECTRAL_LUMINANCE);
    float runtimeLuminance = dot(albedo * irradiance * factor, SPECTRAL_LUMINANCE);
    if (runtimeLuminance > 0.0)
    {
        factor *= targetLuminance / runtimeLuminance;
    }

    return clamp(factor, 0.0, MAX_FACTOR);
}
