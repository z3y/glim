Shader "Hidden/Glim/LightmapTexelGrid"
{
    SubShader
    {
        Tags { "RenderType"="Opaque" }

        Pass
        {
            Tags { "LightMode"="ForwardBase" }

            HLSLPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma target 4.5
            #pragma multi_compile_fwdbase

            #include "UnityCG.cginc"

            struct Attributes
            {
                float3 positionOS : POSITION;
                float3 normalOS : NORMAL;
                float2 uv1 : TEXCOORD1;
            };

            struct Varyings
            {
                float4 positionCS : SV_POSITION;
                float2 lightmapUV : TEXCOORD0;
                float3 positionWS : POSITIONWS;
                float3 normalWS : NORMAL;
            };

            Varyings vert(Attributes attributes)
            {
                Varyings o;

                float3 positionWS = mul(UNITY_MATRIX_M, float4(attributes.positionOS, 1.0)).xyz;
                o.positionCS = mul(UNITY_MATRIX_VP, float4(positionWS, 1.0));
                o.lightmapUV = attributes.uv1 * unity_LightmapST.xy + unity_LightmapST.zw;
                o.positionWS = positionWS;
                o.normalWS = UnityObjectToWorldNormal(attributes.normalOS);

                return o;
            }

            float4 frag(Varyings varyings) : SV_Target
            {
                // headlight shading so edges and corners stay readable from any angle
                float3 normalWS = normalize(varyings.normalWS);
                float3 viewDirectionWS = normalize(_WorldSpaceCameraPos - varyings.positionWS);
                float shading = lerp(0.3, 1.0, abs(dot(normalWS, viewDirectionWS)));

                #ifndef LIGHTMAP_ON
                    // not lightmapped, uses probes or light volumes
                    return float4(float3(0.5, 0.5, 0.5) * shading, 1.0);
                #else
                    float2 lightmapSize;
                    unity_Lightmap.GetDimensions(lightmapSize.x, lightmapSize.y);

                    // one square per lightmap texel, box filtered over the pixel footprint to avoid aliasing
                    // https://iquilezles.org/articles/checkerfiltering/
                    float2 texel = varyings.lightmapUV * lightmapSize;
                    float2 filterWidth = fwidth(texel) + 1e-4;
                    float2 integral = 2.0 * (abs(frac((texel - 0.5 * filterWidth) * 0.5) - 0.5) - abs(frac((texel + 0.5 * filterWidth) * 0.5) - 0.5)) / filterWidth;
                    float checker = 0.5 - 0.5 * integral.x * integral.y;

                    float3 color = lerp(float3(0.0, 0.0, 0.0), float3(0.32, 0.0, 0.32), checker);

                    return float4(color * shading, 1.0);
                #endif
            }

            ENDHLSL
        }
    }
}
