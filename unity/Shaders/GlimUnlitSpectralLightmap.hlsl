#ifndef GLIM_UNLIT_SPECTRAL_LIGHTMAP_INCLUDED
#define GLIM_UNLIT_SPECTRAL_LIGHTMAP_INCLUDED

// Shared by the passes of Glim/Unlit Spectral Lightmap, Built-in and URP.

#include "UnityCG.cginc"
#include "UnityMetaPass.cginc"
#include "GlimSpectral.hlsl"

sampler2D _MainTex;
float4 _MainTex_ST;
float4 _Color;

struct Attributes
{
    float4 positionOS : POSITION;
    float3 normalOS : NORMAL;
    float2 uv0 : TEXCOORD0;
    float2 uv1 : TEXCOORD1;
    float2 uv2 : TEXCOORD2;
    UNITY_VERTEX_INPUT_INSTANCE_ID
};

float4 SampleAlbedo(float2 uv)
{
    return tex2D(_MainTex, uv) * _Color;
}

// Forward

struct Varyings
{
    float4 positionCS : SV_POSITION;
    float2 uv0 : TEXCOORD0;
    float2 lightmapUV : TEXCOORD1;
    float3 normalWS : TEXCOORD2;
    UNITY_VERTEX_OUTPUT_STEREO
};

Varyings vert(Attributes input)
{
    Varyings output;
    UNITY_SETUP_INSTANCE_ID(input);
    UNITY_INITIALIZE_VERTEX_OUTPUT_STEREO(output);

    output.positionCS = UnityObjectToClipPos(input.positionOS);
    output.uv0 = TRANSFORM_TEX(input.uv0, _MainTex);
    output.lightmapUV = input.uv1 * unity_LightmapST.xy + unity_LightmapST.zw;
    output.normalWS = UnityObjectToWorldNormal(input.normalOS);
    return output;
}

float4 frag(Varyings input) : SV_Target
{
    float3 albedo = SampleAlbedo(input.uv0).rgb;
    float3 normalWS = normalize(input.normalWS);

    float3 color;
#if defined(LIGHTMAP_ON)
    float3 lightmap = DecodeLightmap(UNITY_SAMPLE_TEX2D(unity_Lightmap, input.lightmapUV));
    #if defined(DIRLIGHTMAP_COMBINED)
        float4 direction = UNITY_SAMPLE_TEX2D_SAMPLER(unity_LightmapInd, unity_Lightmap, input.lightmapUV);
        lightmap = DecodeDirectionalLightmap(lightmap, direction, normalWS);
    #endif
    #if defined(_GLIM_SPECTRAL_ALBEDO_OFFSET)
        // the spectral albedo offset, see GlimSpectral.hlsl
        color = lightmap * GlimSpectralLightmapAlbedo(albedo);
    #else
        color = lightmap * albedo;
    #endif
#else
    // light probes are not spectrally corrected, plain albedo
    color = ShadeSH9(float4(normalWS, 1.0)) * albedo;
#endif

    return float4(color, 1.0);
}

// Meta, the albedo Glim and Unity bake with

struct VaryingsMeta
{
    float4 positionCS : SV_POSITION;
    float2 uv0 : TEXCOORD0;
};

VaryingsMeta vertMeta(Attributes input)
{
    VaryingsMeta output;
    output.positionCS = UnityMetaVertexPosition(input.positionOS, input.uv1, input.uv2, unity_LightmapST, unity_DynamicLightmapST);
    output.uv0 = TRANSFORM_TEX(input.uv0, _MainTex);
    return output;
}

float4 fragMeta(VaryingsMeta input) : SV_Target
{
    UnityMetaInput meta;
    UNITY_INITIALIZE_OUTPUT(UnityMetaInput, meta);
    meta.Albedo = SampleAlbedo(input.uv0).rgb;
    meta.Emission = 0.0;
    return UnityMetaFragment(meta);
}

#endif
