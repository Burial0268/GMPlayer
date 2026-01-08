export const fragmentShader = `#version 300 es

// This shader creates a multi-layered, collage-like effect by blending
// a blurred background with multiple, sharper, animated versions of the same texture.

precision highp float;
uniform float time;
uniform sampler2D u_texture;
uniform sampler2D u_flowMap;
uniform float u_saturation;
uniform float u_blurLevel;
uniform float u_flowStrength;
uniform float u_distortionStrength;
uniform float u_globalBrightness;
uniform vec3 u_baseColor;
uniform float u_isSmallScreen;

in vec2 v_texCoord;
out vec4 fragColor;

// Smooth cubic ease (Bezier-like).
float bezierEase(float t) {
    t = clamp(t, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// Simple circular influence for mobile - smoother, less complex
float circleInfluence(vec2 uv, vec2 center, float radius, float softness) {
    float dist = length(uv - center);
    float t = 1.0 - smoothstep(0.0, radius, dist);
    return pow(t, softness);
}

// Irregular Bezier-like influence using directional curvature.
float shapeInfluence(vec2 uv, vec2 center, vec2 axisScale, float radius, float harden) {
    vec2 d = uv - center;
    float dist = length(d);
    if (dist < 1e-4) return 1.0;

    float theta = atan(d.y, d.x);
    float dirScale = mix(axisScale.x, axisScale.y, 0.5 + 0.5 * sin(theta * 3.0));

    float normDist = dist / max(radius * dirScale, 1e-3);
    float t = bezierEase(1.0 - normDist);
    return pow(max(t, 0.0), harden);
}

void main() {
    vec2 uv = v_texCoord;

    // Base flow-driven distortion
    vec2 flow = texture(u_flowMap, uv + time * 0.02).xy - 0.5;
    float distortAmount = u_isSmallScreen > 0.5 ? u_distortionStrength * 0.3 : u_distortionStrength;
    vec2 warpedUv = clamp(uv + flow * distortAmount, 0.0, 1.0);

    // Blurred base color
    float baseBlurLod = u_isSmallScreen > 0.5 ? u_blurLevel * 0.8 + 3.5 : u_blurLevel * 0.6 + 2.0;
    vec3 sampled = textureLod(u_texture, warpedUv, max(0.0, baseBlurLod)).rgb;
    vec3 base_color = mix(u_baseColor, sampled, 0.55);

    vec3 final_rgb;

    if (u_isSmallScreen > 0.5) {
        // ============ MOBILE PATH: Simpler, smoother effect ============

        float slowTime = time * 0.6;

        vec2 pos1 = vec2(0.5) + (texture(u_flowMap, vec2(slowTime * 0.02, 0.3)).xy - 0.5) * 0.45;
        vec2 pos2 = vec2(0.5) + (texture(u_flowMap, vec2(0.7, slowTime * 0.018)).xy - 0.5) * 0.4;

        vec2 centerPull = vec2(0.5);
        pos1 = mix(pos1, centerPull, 0.3);
        pos2 = mix(pos2, centerPull, 0.35);

        // Higher blur for smoother color sampling on mobile
        float mobileBlurLod = u_blurLevel * 0.6 + 4.0;
        vec3 color1 = textureLod(u_texture, pos1, max(0.0, mobileBlurLod)).rgb;
        vec3 color2 = textureLod(u_texture, pos2, max(0.0, mobileBlurLod)).rgb;

        float r1 = 0.95;
        float r2 = 0.88;

        float influence1 = circleInfluence(uv, pos1, r1, 0.8);
        float influence2 = circleInfluence(uv, pos2, r2, 0.85);

        float total_influence = influence1 + influence2;

        vec3 mixed_color = (influence1 * color1 + influence2 * color2) / max(total_influence, 1e-3);

        float blend_factor = smoothstep(0.05, 0.98, total_influence);
        final_rgb = mix(base_color, mixed_color, blend_factor);

        final_rgb = mix(vec3(0.5), final_rgb, 1.02);
        final_rgb *= u_globalBrightness;

    } else {
        // ============ DESKTOP PATH: Original complex effect ============

        vec2 pos1 = vec2(0.5) + (texture(u_flowMap, vec2(time * 0.031, 0.27)).xy - 0.5) * (0.65 + u_flowStrength);
        vec2 pos2 = vec2(0.5) + (texture(u_flowMap, vec2(0.83, time * 0.023)).xy - 0.5) * (0.55 + u_flowStrength * 0.7);
        vec2 pos3 = vec2(0.5) + (texture(u_flowMap, vec2(time * 0.017, 0.63 + time * 0.009)).xy - 0.5) * (0.64 + u_flowStrength * 0.85);

        vec2 centerPull = vec2(0.5);
        pos1 = mix(pos1, centerPull, 0.08);
        pos2 = mix(pos2, centerPull, 0.08);
        pos3 = mix(pos3, centerPull, 0.08);

        vec2 axis1 = 1.0 + (texture(u_flowMap, pos1 * 0.7 + time * 0.01).xy - 0.5) * 0.6;
        vec2 axis2 = 1.0 + (texture(u_flowMap, pos2 * 0.82 + time * 0.013).xy - 0.5) * 0.58;
        vec2 axis3 = 1.0 + (texture(u_flowMap, pos3 * 0.78 + time * 0.011).xy - 0.5) * 0.62;

        float desktopBlurLod = u_blurLevel * 0.35 + 1.0;
        vec3 color1 = textureLod(u_texture, pos1, max(0.0, desktopBlurLod)).rgb;
        vec3 color2 = textureLod(u_texture, pos2, max(0.0, desktopBlurLod)).rgb;
        vec3 color3 = textureLod(u_texture, pos3, max(0.0, desktopBlurLod)).rgb;

        float r1 = 0.52;
        float r2 = 0.44;
        float r3 = 0.48;

        float influence1 = shapeInfluence(uv, pos1, axis1, r1, 1.35);
        float influence2 = shapeInfluence(uv, pos2, axis2, r2, 1.3);
        float influence3 = shapeInfluence(uv, pos3, axis3, r3, 1.32);

        float total_influence = influence1 + influence2 + influence3;

        vec3 mixed_color = (influence1 * color1 + influence2 * color2 + influence3 * color3) / max(total_influence, 1e-3);

        float blend_factor = smoothstep(0.25, 0.85, total_influence);

        final_rgb = mix(base_color, mixed_color, blend_factor);
        final_rgb = mix(vec3(0.5), final_rgb, 1.05);
        final_rgb *= u_globalBrightness;
    }

    // ============ Common post-processing ============

    float luma = dot(final_rgb, vec3(0.299, 0.587, 0.114));
    vec3 desaturatedColor = vec3(luma);
    final_rgb = mix(desaturatedColor, final_rgb, u_saturation);

    float maxc = max(max(final_rgb.r, final_rgb.g), final_rgb.b);
    float minc = min(min(final_rgb.r, final_rgb.g), final_rgb.b);
    float chroma = maxc - minc;
    float sat = chroma / max(maxc, 1e-3);
    float satLimiter = smoothstep(1.2, 1.6, sat);
    vec3 lumaMix = mix(vec3(luma), final_rgb, 0.65);
    final_rgb = mix(final_rgb, lumaMix, satLimiter * 0.35);

    float baseLuma = dot(base_color, vec3(0.299, 0.587, 0.114));
    float lumaDiff = abs(luma - baseLuma);
    float harmony = smoothstep(0.15, 0.35, lumaDiff);
    final_rgb = mix(final_rgb, mix(final_rgb, base_color, 0.4), harmony);

    vec3 n_final = normalize(max(final_rgb, 1e-4));
    vec3 n_base = normalize(max(base_color, 1e-4));
    float hueDiff = acos(clamp(dot(n_final, n_base), -1.0, 1.0)) / 3.1415926;
    float hueBlend = smoothstep(0.18, 0.45, hueDiff);
    final_rgb = mix(final_rgb, mix(final_rgb, base_color, 0.5), hueBlend * 0.4);

    final_rgb = mix(final_rgb, mix(base_color, final_rgb, 0.78), 0.1);

    float finalLuma = dot(final_rgb, vec3(0.299, 0.587, 0.114));
    float targetLuma = max(baseLuma * 0.9, 0.34);
    float lumaLift = max(0.0, targetLuma - finalLuma);
    final_rgb += lumaLift * 0.8;

    final_rgb *= 1.18;
    final_rgb = 1.0 - exp(-final_rgb);

    fragColor = vec4(final_rgb, 1.0);
}
`;
