// Unlit, only baked lighting: the lightmap times albedo with the Glim spectral albedo offset.
// Bake with "Spectral Albedo Offset" on the Glim Lightmapper, see GlimSpectral.hlsl.
// The material toggle switches back to the plain albedo multiply, for lightmaps baked without it.
Shader "Glim/Unlit Spectral Lightmap"
{
    Properties
    {
        _Color ("Color", Color) = (1, 1, 1, 1)
        _MainTex ("Albedo", 2D) = "white" {}
        [Toggle(_GLIM_SPECTRAL_ALBEDO_OFFSET)] _GlimSpectralAlbedoOffset ("Spectral Albedo Offset", Float) = 1
    }

    // URP
    SubShader
    {
        Tags { "RenderType" = "Opaque" "RenderPipeline" = "UniversalPipeline" }

        Pass
        {
            Name "Forward"
            Tags { "LightMode" = "UniversalForward" }

            HLSLPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma multi_compile _ LIGHTMAP_ON
            #pragma multi_compile _ DIRLIGHTMAP_COMBINED
            #pragma multi_compile_instancing
            #pragma shader_feature_local _GLIM_SPECTRAL_ALBEDO_OFFSET
            #include "GlimUnlitSpectralLightmap.hlsl"
            ENDHLSL
        }

        Pass
        {
            Name "META"
            Tags { "LightMode" = "Meta" }
            Cull Off

            HLSLPROGRAM
            #pragma vertex vertMeta
            #pragma fragment fragMeta
            #include "GlimUnlitSpectralLightmap.hlsl"
            ENDHLSL
        }
    }

    // Built-in
    SubShader
    {
        Tags { "RenderType" = "Opaque" }

        Pass
        {
            Name "FORWARD"
            Tags { "LightMode" = "ForwardBase" }

            HLSLPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma multi_compile _ LIGHTMAP_ON
            #pragma multi_compile _ DIRLIGHTMAP_COMBINED
            #pragma multi_compile_instancing
            #pragma shader_feature_local _GLIM_SPECTRAL_ALBEDO_OFFSET
            #include "GlimUnlitSpectralLightmap.hlsl"
            ENDHLSL
        }

        Pass
        {
            Name "META"
            Tags { "LightMode" = "Meta" }
            Cull Off

            HLSLPROGRAM
            #pragma vertex vertMeta
            #pragma fragment fragMeta
            #include "GlimUnlitSpectralLightmap.hlsl"
            ENDHLSL
        }
    }
}
