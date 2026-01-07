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

in vec2 v_texCoord;
out vec4 fragColor;

// Smooth cubic ease (Bezier-like).
float bezierEase(float t) {
    t = clamp(t, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// Irregular Bezier-like influence using directional curvature.
float shapeInfluence(vec2 uv, vec2 center, vec2 axisScale, float radius, float harden) {
    vec2 d = uv - center;
    float dist = length(d);
    if (dist < 1e-4) return 1.0;

    float theta = atan(d.y, d.x);
    // Sinusoidal mix to morph the contour; more organic than pure ellipse.
    float dirScale = mix(axisScale.x, axisScale.y, 0.5 + 0.5 * sin(theta * 3.0));

    float normDist = dist / max(radius * dirScale, 1e-3);
    float t = bezierEase(1.0 - normDist);
    return pow(max(t, 0.0), harden);
}

void main() {
    vec2 uv = v_texCoord;

    // Base flow-driven distortion to avoid a static feel.
    vec2 flow = texture(u_flowMap, uv + time * 0.02).xy - 0.5;
    vec2 warpedUv = clamp(uv + flow * u_distortionStrength, 0.0, 1.0);

    // A soft color from the album art blended with pre-picked base to reduce偏色
    vec3 sampled = textureLod(u_texture, warpedUv, max(0.0, u_blurLevel * 0.6 + 2.0)).rgb;
    vec3 base_color = mix(u_baseColor, sampled, 0.55);

    // Define positions for our metaballs.
    // Their movement is driven by sampling the fBm noise (flowMap) at different, slow-moving points.
    vec2 pos1 = vec2(0.5) + (texture(u_flowMap, vec2(time * 0.03, 0.2)).xy - 0.5) * (0.65 + u_flowStrength);
    vec2 pos2 = vec2(0.5) + (texture(u_flowMap, vec2(0.8, time * 0.02)).xy - 0.5) * (0.55 + u_flowStrength * 0.7);
    vec2 pos3 = vec2(0.5) + (texture(u_flowMap, vec2(time * 0.01, time * 0.04)).xy - 0.5) * (0.6 + u_flowStrength * 0.9);

    // Directional scale from flow map to create irregular Bezier-like blobs.
    vec2 axis1 = 1.0 + (texture(u_flowMap, pos1 * 0.7 + time * 0.01).xy - 0.5) * 0.6;
    vec2 axis2 = 1.0 + (texture(u_flowMap, pos2 * 0.8 + time * 0.012).xy - 0.5) * 0.55;
    vec2 axis3 = 1.0 + (texture(u_flowMap, pos3 * 0.75 + time * 0.009).xy - 0.5) * 0.58;

    // Extract colors for each metaball.
    // We sample from a blurred version of the texture (using textureLod) to prevent flickering
    // as the sample position moves over detailed parts of the album art.
    vec3 color1 = textureLod(u_texture, pos1, max(0.0, u_blurLevel * 0.35 + 1.0)).rgb;
    vec3 color2 = textureLod(u_texture, pos2, max(0.0, u_blurLevel * 0.35 + 1.0)).rgb;
    vec3 color3 = textureLod(u_texture, pos3, max(0.0, u_blurLevel * 0.35 + 1.0)).rgb;

    // Define the "mass" or radius of each metaball
    float r1 = 0.52;
    float r2 = 0.44;
    float r3 = 0.48;

    // Calculate the influence of each metaball on the current pixel.
    float influence1 = shapeInfluence(uv, pos1, axis1, r1, 1.35);
    float influence2 = shapeInfluence(uv, pos2, axis2, r2, 1.3);
    float influence3 = shapeInfluence(uv, pos3, axis3, r3, 1.32);
    
    // Sum the influences to create a combined field.
    float total_influence = influence1 + influence2 + influence3;

    // Blend the metaball colors based on their relative influence at this pixel.
    vec3 mixed_color = (influence1 * color1 + influence2 * color2 + influence3 * color3) / max(total_influence, 1e-3);

    // Now, blend the final metaball color with the soft base color.
    // The smoothstep creates the soft, volumetric shape of the combined metaballs.
    // Use a softer threshold tuned for Bezier-like falloff.
    float blend_factor = smoothstep(0.25, 0.85, total_influence);
    
    vec3 final_rgb = mix(base_color, mixed_color, blend_factor);
    // Subtle contrast & brightness lift to avoid flat look.
    final_rgb = mix(vec3(0.5), final_rgb, 1.05);
    final_rgb *= u_globalBrightness;

    // Final saturation adjustment
    float luma = dot(final_rgb, vec3(0.299, 0.587, 0.114));
    vec3 desaturatedColor = vec3(luma);
    final_rgb = mix(desaturatedColor, final_rgb, u_saturation);

    // Guard rails to避免不和谐拼色：限制过高饱和度并回归基色。
    float maxc = max(max(final_rgb.r, final_rgb.g), final_rgb.b);
    float minc = min(min(final_rgb.r, final_rgb.g), final_rgb.b);
    float chroma = maxc - minc;
    float sat = chroma / max(maxc, 1e-3);
    float satLimiter = smoothstep(1.2, 1.6, sat); // high sat -> stronger clamp
    vec3 lumaMix = mix(vec3(luma), final_rgb, 0.65);
    final_rgb = mix(final_rgb, lumaMix, satLimiter * 0.35);

    // Luma harmony with base_color to reduce patchy blends.
    float baseLuma = dot(base_color, vec3(0.299, 0.587, 0.114));
    float lumaDiff = abs(luma - baseLuma);
    float harmony = smoothstep(0.15, 0.35, lumaDiff);
    final_rgb = mix(final_rgb, mix(final_rgb, base_color, 0.4), harmony);

    // Smooth transition between discordant hues: blend toward base when hue vectors diverge.
    vec3 n_final = normalize(max(final_rgb, 1e-4));
    vec3 n_base = normalize(max(base_color, 1e-4));
    float hueDiff = acos(clamp(dot(n_final, n_base), -1.0, 1.0)) / 3.1415926; // 0..1
    float hueBlend = smoothstep(0.18, 0.45, hueDiff);
    final_rgb = mix(final_rgb, mix(final_rgb, base_color, 0.5), hueBlend * 0.4);

    // Subtle low-pass blend to avoid patchy transitions.
    final_rgb = mix(final_rgb, mix(base_color, final_rgb, 0.78), 0.1);

    // Prevent dark sinkholes: pull luma toward base if too low.
    float finalLuma = dot(final_rgb, vec3(0.299, 0.587, 0.114));
    float targetLuma = max(baseLuma * 0.9, 0.34);
    float lumaLift = max(0.0, targetLuma - finalLuma);
    final_rgb += lumaLift * 0.8;

    // Brighten a bit before tone map，减轻整体发暗。
    final_rgb *= 1.18;
    // Soft tone map保持高光但避免溢出。
    final_rgb = 1.0 - exp(-final_rgb);

    fragColor = vec4(final_rgb, 1.0);
}
`; 