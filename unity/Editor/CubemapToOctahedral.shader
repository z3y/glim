Shader "Hidden/Glim/CubemapToOctahedral"
{
    Properties
    {
        _Cube ("Cube", Cube) = "" {}
    }
    SubShader
    {
        Cull Off ZWrite Off ZTest Always
        Pass
        {
            CGPROGRAM
            #pragma vertex vert_img
            #pragma fragment frag
            #include "UnityCG.cginc"

            samplerCUBE _Cube;

            float3 OctDecode(float2 uv)
            {
                float2 f = uv * 2.0 - 1.0;
                float3 n = float3(f.x, f.y, 1.0 - abs(f.x) - abs(f.y));
                float t = saturate(-n.z);
                n.xy += float2(n.x >= 0.0 ? -t : t, n.y >= 0.0 ? -t : t);
                return normalize(n);
            }

            float4 frag(v2f_img i) : SV_Target
            {
                float3 dir = OctDecode(i.uv);
                return texCUBElod(_Cube, float4(dir, 0));
            }
            ENDCG
        }
    }
}