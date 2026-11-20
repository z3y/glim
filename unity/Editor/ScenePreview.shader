// Draws the path traced preview over the whole scene view, before post processing so it still applies.
// The queue and depth write are set by ScenePreview.cs per render pipeline.
Shader "Hidden/Glim/Scene Preview"
{
    Properties
    {
        _MainTex ("Preview", 2D) = "black" {}
        // 1: writes near plane depth so nothing drawn after the quad (skybox, transparents) covers it.
        // the camera depth texture is already rendered at that point, so effects using it keep the scene depth
        [Toggle] _ZWrite ("Write Near Depth", Float) = 0
    }
    SubShader
    {
        Tags { "Queue"="Overlay" "RenderType"="Overlay" "IgnoreProjector"="True" "PreviewType"="Plane" }

        Pass
        {
            ZTest Always
            ZWrite [_ZWrite]
            Cull Off
            Blend Off

            HLSLPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma target 3.5
            // URP global keyword, set while Screen Space Ambient Occlusion is active (not in After Opaque mode)
            #pragma multi_compile _ _SCREEN_SPACE_OCCLUSION

            #include "UnityCG.cginc"

            Texture2D _MainTex;
            SamplerState sampler_MainTex;
            float _ZWrite;

            #ifdef _SCREEN_SPACE_OCCLUSION
            // URP globals, same as URP's AmbientOcclusion.hlsl
            Texture2D _ScreenSpaceOcclusionTexture;
            SamplerState sampler_ScreenSpaceOcclusionTexture;
            float4 _AmbientOcclusionParam;
            float4 _ScaledScreenParams;
            #endif

            struct Attributes
            {
                // quad corners in [-1, 1], already in clip space
                float3 positionOS : POSITION;
            };

            struct Varyings
            {
                float4 positionCS : SV_POSITION;
                float2 uv : TEXCOORD0;
            };

            Varyings vert(Attributes input)
            {
                Varyings o;
                float2 ndc = input.positionOS.xy;
                // _ProjectionParams.x is -1 when unity renders with a flipped projection
                // 0.5 depth is inside the clip range for both conventions and reversed z,
                // the near plane value is slightly pulled in so it doesn't get clipped
                float z = _ZWrite > 0.5 ? UNITY_NEAR_CLIP_VALUE * 0.9999 : 0.5;
                o.positionCS = float4(ndc.x, ndc.y * _ProjectionParams.x, z, 1.0);
                // the first row of the readback is the top of the image, unity textures start at the bottom
                o.uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
                return o;
            }

            float4 frag(Varyings input) : SV_Target
            {
                float3 color = _MainTex.SampleLevel(sampler_MainTex, input.uv, 0).rgb;

                // a single NaN or inf would spread over the whole screen with bloom
                color = any(isnan(color)) ? 0.0 : min(color, 65000.0);

                #ifdef _SCREEN_SPACE_OCCLUSION
                // URP lit shaders apply SSAO themselves, the path traced image doesn't separate direct
                // and indirect light so it's applied to everything, like the After Opaque mode does
                float2 screenUV = input.positionCS.xy / _ScaledScreenParams.xy;
                float ao = _ScreenSpaceOcclusionTexture.SampleLevel(sampler_ScreenSpaceOcclusionTexture, screenUV, 0).x;
                color *= saturate(ao + (1.0 - _AmbientOcclusionParam.x));
                #endif

                #ifdef UNITY_COLORSPACE_GAMMA
                color = LinearToGammaSpace(color);
                #endif

                return float4(color, 1.0);
            }
            ENDHLSL
        }
    }
}
