Shader "Hidden/Glim/TerrainMeta"
{
    Properties
    {
        // _MainTex("Albedo", 2D) = "white" {}
        // _Splat ("Splat", 2D) = "black" {}
        // _SplatChannel("Control Channel", Integer) = 0
    }
    SubShader
    {
        Tags { "RenderType"="Transparent" }

        Pass
        {
            Cull Off
            Blend One One

            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma target 4.5

            #include "UnityCG.cginc"

            struct appdata
            {
                float4 vertex : POSITION;
                float2 uv : TEXCOORD0;
            };

            struct v2f
            {
                float2 uv : TEXCOORD0;
                float4 vertex : SV_POSITION;
            };

            Texture2D _MainTex;
            SamplerState sampler_MainTex;

            Texture2D _Splat;
            SamplerState sampler_Splat;
            uint _SplatChannel;

            float4 _MainTex_ST;

            v2f vert (appdata v)
            {
                v2f o;
                float2 lightmapUV = v.uv;
                o.vertex = float4(lightmapUV * 2.0 - 1.0, 0, 1);

                o.uv = v.uv;
                return o;
            }

            float4 frag (v2f i) : SV_Target
            {
                float4 col = _MainTex.SampleLevel(sampler_MainTex, mad(i.uv, _MainTex_ST.xy, _MainTex_ST.zw), 0);

                float mask = _Splat.SampleLevel(sampler_Splat, i.uv, 0)[_SplatChannel];
                col.rgb *= mask;
                col.a = 1;

                return col;
            }
            ENDCG
        }
    }
}
