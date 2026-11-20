// Built-in pipeline test shader for neural lightmaps (Lightmap Mode: Neural)
// Unlit apart from the baked light: albedo * neural diffuse (with normal map) + neural specular
// Heavy: every network evaluation reads ~1900 weights, the blended mode evaluates it 8 times per pixel

Shader "Glim/Neural Lightmap Unlit"
{
    Properties
    {
        _MainTex ("Albedo", 2D) = "white" {}
        _Color ("Color", Color) = (1, 1, 1, 1)
        [Normal][NoScaleOffset] _BumpMap ("Normal Map", 2D) = "bump" {}
        _BumpScale ("Normal Scale", Float) = 1
        _Roughness ("Roughness", Range(0, 1)) = 0.4
        _SpecularIntensity ("Specular Intensity", Range(0, 1)) = 1
        [Enum(Blend 4 Evaluations, 0, Bilinear Features, 1)] _Filtering ("Filtering", Float) = 0

        [Header(Debug)]
        [Enum(Lit, 0, Regular Lightmap, 1, Neural Diffuse, 2, Neural Specular, 3, Neural Diffuse vs Lightmap, 4, Features, 5)] _DebugView ("Debug View", Float) = 0
        [IntRange] _DebugFeatures ("Debug Features Texture", Range(0, 3)) = 0
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
            #pragma target 4.5
            #pragma multi_compile _ LIGHTMAP_ON

            #include "UnityCG.cginc"
            #include "Packages/io.github.z3y.glim/ShaderLibrary/GlimNeuralLightmap.hlsl"

            sampler2D _MainTex;
            float4 _MainTex_ST;
            sampler2D _BumpMap;
            float _BumpScale;
            float4 _Color;
            float _Roughness;
            float _SpecularIntensity;
            float _Filtering;
            float _DebugView;
            float _DebugFeatures;

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

            float3 SampleNeural(float2 lightmapUV, float3 direction, float lobe)
            {
                if (_Filtering > 0.5)
                {
                    return GlimNNSampleBilinearFeatures(lightmapUV, direction, lobe);
                }
                return GlimNNSampleBlended(lightmapUV, direction, lobe);
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
                    return float4(DecodeLightmap(UNITY_SAMPLE_TEX2D(unity_Lightmap, i.lightmapUV)), 1);
                }
                if (view == 5)
                {
                    GlimNNTaps taps = GlimNNGetTaps(i.lightmapUV);
                    GlimNNFeatures f = GlimNNLoadTap(taps, int2(0, 0));
                    float4 v = f.f[clamp((int)_DebugFeatures, 0, 3)];
                    return float4(v.rgb * 2.0 + 0.5, 1);
                }

                float3 diffuse = SampleNeural(i.lightmapUV, normal, GLIM_NN_LOBE_COSINE);

                if (view == 2)
                {
                    return float4(diffuse, 1);
                }
                if (view == 4)
                {
                    // left of the screen the network, right the regular lightmap, compare without normal maps
                    float3 lightmap = DecodeLightmap(UNITY_SAMPLE_TEX2D(unity_Lightmap, i.lightmapUV));
                    float3 geometric = SampleNeural(i.lightmapUV, normalize(i.normalWS), GLIM_NN_LOBE_COSINE);
                    float split = i.pos.x / _ScreenParams.x;
                    return float4(split < 0.5 ? geometric : lightmap, 1);
                }

                float3 R = reflect(-viewDir, normal);
                float3 specular = SampleNeural(i.lightmapUV, R, GlimNNSpecularLobe(_Roughness, NoV));
                specular *= EnvBRDFApprox(0.04, _Roughness, NoV) * _SpecularIntensity;

                if (view == 3)
                {
                    return float4(specular, 1);
                }

                return float4(albedo * diffuse + specular, 1);
            #else
                return float4(albedo * ShadeSH9(float4(normal, 1)), 1);
            #endif
            }
            ENDCG
        }
    }
}
