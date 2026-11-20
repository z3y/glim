// Minimal Built-in pipeline test shader for Spherical Gaussian lightmaps (Lightmap Mode: Spherical Gaussian)
// Unlit apart from the baked light: albedo * SG diffuse (with normal map) + SG specular

Shader "Glim/SG Lightmap Unlit"
{
    Properties
    {
        _MainTex ("Albedo", 2D) = "white" {}
        _Color ("Color", Color) = (1, 1, 1, 1)
        [Normal][NoScaleOffset] _BumpMap ("Normal Map", 2D) = "bump" {}
        _BumpScale ("Normal Scale", Float) = 1
        _Roughness ("Roughness", Range(0, 1)) = 0.5
        _SpecularIntensity ("Specular Intensity", Range(0, 1)) = 1

        [Header(Debug)]
        [Enum(Lit, 0, Regular Lightmap Tile, 1, SG Diffuse, 2, SG Specular, 3, Single Lobe, 4, Whole Texture, 5)] _DebugView ("Debug View", Float) = 0
        [IntRange] _DebugLobe ("Debug Lobe", Range(0, 7)) = 0
    }

    SubShader
    {
        Tags { "RenderType" = "Opaque" }

        Pass
        {
            Tags { "LightMode" = "ForwardBase" }

            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma target 3.5
            #pragma multi_compile _ LIGHTMAP_ON

            #include "UnityCG.cginc"

            // handles every lightmap encoding, a no-op for High Quality (HDR)
            #define GLIM_SG_DECODE(c) DecodeLightmap(c)
            #include "Packages/io.github.z3y.glim/ShaderLibrary/GlimSGLightmap.hlsl"

            sampler2D _MainTex;
            float4 _MainTex_ST;
            sampler2D _BumpMap;
            float _BumpScale;
            float4 _Color;
            float _Roughness;
            float _SpecularIntensity;
            float _DebugView;
            float _DebugLobe;

            struct appdata
            {
                float4 vertex : POSITION;
                float3 normal : NORMAL;
                float4 tangent : TANGENT;
                float2 uv : TEXCOORD0;
                float2 uv1 : TEXCOORD1;
            };

            struct v2f
            {
                float4 pos : SV_POSITION;
                float2 uv : TEXCOORD0;
                float2 lightmapUV : TEXCOORD1;
                float3 positionWS : TEXCOORD2;
                float3 normalWS : TEXCOORD3;
                float4 tangentWS : TEXCOORD4;
            };

            v2f vert(appdata v)
            {
                v2f o;
                o.pos = UnityObjectToClipPos(v.vertex);
                o.uv = TRANSFORM_TEX(v.uv, _MainTex);
                // points into the first tile, the regular lightmap
                o.lightmapUV = v.uv1 * unity_LightmapST.xy + unity_LightmapST.zw;
                o.positionWS = mul(unity_ObjectToWorld, v.vertex).xyz;
                o.normalWS = UnityObjectToWorldNormal(v.normal);
                o.tangentWS = float4(UnityObjectToWorldDir(v.tangent.xyz), v.tangent.w * unity_WorldTransformParams.w);
                return o;
            }

            // Karis, mobile approximation of the split sum environment BRDF
            float3 EnvBRDFApprox(float3 specularColor, float roughness, float NoV)
            {
                const float4 c0 = float4(-1, -0.0275, -0.572, 0.022);
                const float4 c1 = float4(1, 0.0425, 1.04, -0.04);
                float4 r = roughness * c0 + c1;
                float a004 = min(r.x * r.x, exp2(-9.28 * NoV)) * r.x + r.y;
                float2 AB = float2(-1.04, 1.04) * a004 + r.zw;
                return specularColor * AB.x + AB.y;
            }

            float4 frag(v2f i) : SV_Target
            {
                float3 albedo = tex2D(_MainTex, i.uv).rgb * _Color.rgb;

                float3 tangentNormal = UnpackNormal(tex2D(_BumpMap, i.uv));
                tangentNormal.xy *= _BumpScale;
                float3 normal = normalize(i.normalWS);
                float3 tangent = normalize(i.tangentWS.xyz - normal * dot(normal, i.tangentWS.xyz));
                float3 bitangent = cross(normal, tangent) * i.tangentWS.w;
                normal = normalize(tangentNormal.x * tangent + tangentNormal.y * bitangent + tangentNormal.z * normal);

                float3 viewDir = normalize(_WorldSpaceCameraPos - i.positionWS);
                float NoV = saturate(dot(normal, viewDir));

            #ifdef LIGHTMAP_ON
                int view = (int)_DebugView;

                if (view == 1)
                {
                    return float4(GlimSampleSGDiffuseTile(unity_Lightmap, samplerunity_Lightmap, i.lightmapUV), 1);
                }
                if (view == 5)
                {
                    // the whole 3x3 grid stretched over the chart: the regular lightmap bottom left, the 8 lobes after it row by row
                    float2 uv = i.lightmapUV * GLIM_SG_TILES;
                    return float4(DecodeLightmap(unity_Lightmap.SampleLevel(samplerunity_Lightmap, uv, 0)), 1);
                }

                GlimSG sg = GlimSampleSG(unity_Lightmap, samplerunity_Lightmap, i.lightmapUV);

                if (view == 4)
                {
                    uint lobe = (uint)_DebugLobe;
                    float3 amplitude = 0;
                    [unroll]
                    for (uint k = 0; k < GLIM_SG_LOBE_COUNT; k++)
                    {
                        amplitude = k == lobe ? sg.amplitude[k] : amplitude;
                    }
                    return float4(amplitude, 1);
                }

                float3 diffuse = GlimSGDiffuse(sg, normal);
                float3 specular = GlimSGSpecular(sg, normal, viewDir, _Roughness);
                specular *= EnvBRDFApprox(0.04, _Roughness, NoV) * _SpecularIntensity;

                if (view == 2)
                {
                    return float4(diffuse, 1);
                }
                if (view == 3)
                {
                    return float4(specular, 1);
                }

                return float4(albedo * diffuse + specular, 1);
            #else
                // not lightmapped, light probes
                return float4(albedo * ShadeSH9(float4(normal, 1)), 1);
            #endif
            }
            ENDCG
        }
    }
}
