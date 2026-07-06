Shader "Unlit/Light Mesh"
{
    Properties
    {
        [Enum(Point, 0, Spot, 1, Directional, 2, Area, 3)] _LightType ("Light Type", Int) = 0
        _LightColor ("Light Color", Color) = (1,1,1,1)
        _LightIntensity ("Light Intensity", Float) = 1.0
        _LightSpotAngle ("Spot Angle", Range(0, 179)) = 30
        _LightDirectionalAngle ("Directional Angle", Float) = 0.526
        // todo light range
    }
    SubShader
    {
        Tags { "RenderType"="Opaque" }
        Cull Front

        Pass
        {
            HLSLPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma target 5.0

            #include "UnityCG.cginc"

            uint _LightType;
            float3 _LightColor;
            float _LightIntensity;
            float _LightSpotAngle;
            float _LightDirectionalAngle;

            struct Attributes
            {
                float3 positionOS : POSITION;
                float2 uv0 : TEXCOORD0;
            };

            struct Varyings
            {
                float4 positionCS : SV_POSITION;
                float3 positionWS : POSITIONWS;
            };

            float3 CameraPositionWS()
            {
                return UNITY_MATRIX_I_V._m03_m13_m23;
            }

            Varyings vert(Attributes attributes)
            {
                Varyings o;

                float3 positionOS = attributes.positionOS;
                if (_LightType == 3)
                {
                    positionOS.z = 0.0;
                }

                float3 positionWS = mul(UNITY_MATRIX_M, float4(positionOS, 1.0)).xyz;
                o.positionCS = mul(UNITY_MATRIX_VP, float4(positionWS, 1.0));

                o.positionWS = positionWS;

                return o;
            }

            // https://iquilezles.org/articles/intersectors/
            float SphereIntersect(float3 ro, float3 rd, float4 sph)
            {
                float3 oc = ro - sph.xyz;
                float b = dot( oc, rd );
                float c = dot( oc, oc ) - sph.w*sph.w;
                float h = b*b - c;
                if( h<0.0 ) return -1.0;
                h = sqrt( h );
                return -b - h;
            }

            float GetSpotAngleAttenuation(float3 spotForward, float3 l, float spotScale, float spotOffset)
            {
                float cd = dot(-spotForward, l);
                float attenuation = saturate(cd * spotScale + spotOffset);
                return attenuation * attenuation;
            }

            void GetSpotScaleOffset(float outerAngle, float innerAnglePercent, out float spotScale, out float spotOffset)
            {
                float innerAngle = outerAngle / 100 * innerAnglePercent;
                innerAngle = innerAngle / 360 * UNITY_PI;
                outerAngle = outerAngle / 360 * UNITY_PI;
                float cosOuter = cos(outerAngle);
                spotScale = 1.0 / max(cos(innerAngle) - cosOuter, 1e-4);
                spotOffset = -cosOuter * spotScale;
            }

            struct FragOutput
            {
                float4 color : SV_Target;
                float depth : SV_DepthGreaterEqual;
            };

            FragOutput frag(Varyings varyings)
            {
                float3 lightPosition = UNITY_MATRIX_M._m03_m13_m23;

                float objectScale = float3(
                    length(UNITY_MATRIX_M._m00_m10_m20),
                    length(UNITY_MATRIX_M._m01_m11_m21),
                    length(UNITY_MATRIX_M._m02_m12_m22)
                );

                float3 objectForward = -normalize(UNITY_MATRIX_M._m02_m12_m22);

                float3 ro = CameraPositionWS();
                float3 rd = -normalize(ro - varyings.positionWS);

                float radius = objectScale * 0.5;

                if (_LightType == 2)
                {
                    float dist = _ProjectionParams.z;
                    lightPosition = CameraPositionWS() + objectForward * dist;
                    radius = dist * tan(radians(_LightDirectionalAngle * 0.5));
                }

                float t = SphereIntersect(ro, rd, float4(lightPosition, radius));

                float3 positionWS = ro + rd * t;

                if (_LightType == 3)
                {
                    positionWS = varyings.positionWS;
                    float3 toCam = normalize(CameraPositionWS() - positionWS);
                    bool facingCam = dot(objectForward, toCam) < 0.0;

                    t = facingCam ? 0 : -1;
                }

                if (_LightType == 1)
                {
                    float spotScale;
                    float spotOffset;
                    GetSpotScaleOffset(_LightSpotAngle, 100, spotScale, spotOffset);
                    float3 l = normalize(positionWS - ro);
                    float spotAttenuation = GetSpotAngleAttenuation(-objectForward, l, spotScale, spotOffset);

                    if (spotAttenuation <= 0)
                    {
                        t = -1.0;
                    }
                }

                if (t <= -1.0)
                {
                    discard;
                }

                float4 color = 1.0;

                // todo verify this
                float intensityScale = 1.0;
                if (_LightType == 0 || _LightType == 1)
                {
                    intensityScale = (4.0 * UNITY_PI) * radius * radius;
                }
                if (_LightType == 2)
                {
                    float theta = radians(_LightDirectionalAngle * 0.5);
                    float solidAngle = 2.0 * UNITY_PI * (1.0 - cos(theta));
                    intensityScale = solidAngle;
                }
                if (_LightType == 3)
                {
                    float area = length(UNITY_MATRIX_M._m00_m10_m20) * length(UNITY_MATRIX_M._m01_m11_m21);
                    intensityScale = UNITY_PI * area;
                }

                color.rgb = _LightColor.rgb * _LightIntensity / max(intensityScale, 0.0001);

                float4 positionCS = mul(UNITY_MATRIX_VP, float4(positionWS, 1.0));
                float ndcDepth = positionCS.z / positionCS.w;
                #if defined(SHADER_API_GLCORE) || defined(SHADER_API_OPENGL) || defined(SHADER_API_GLES) || defined(SHADER_API_GLES3)
                    ndcDepth = ndcDepth * 0.5 + 0.5;
                #endif

                FragOutput o;
                o.color = color;
                o.depth = ndcDepth;
                return o;
            }

            ENDHLSL
        }
    }
}
