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

    // Extract colors for each metaball.
    // We sample from a blurred version of the texture (using textureLod) to prevent flickering
    // as the sample position moves over detailed parts of the album art.
    vec3 color1 = textureLod(u_texture, pos1, max(0.0, u_blurLevel * 0.35 + 1.0)).rgb;
    vec3 color2 = textureLod(u_texture, pos2, max(0.0, u_blurLevel * 0.35 + 1.0)).rgb;
    vec3 color3 = textureLod(u_texture, pos3, max(0.0, u_blurLevel * 0.35 + 1.0)).rgb;

    // Define the "mass" or radius of each metaball
    float r1 = 0.5;
    float r2 = 0.4;
    float r3 = 0.45;

    // Calculate the influence of each metaball on the current pixel.
    // We use the inverse square law (r^2 / d^2) for a nice, smooth falloff.
    // dot(v,v) is a cheaper way of calculating length(v)^2.
    float influence1 = r1 * r1 / max(dot(uv - pos1, uv - pos1), 1e-3);
    float influence2 = r2 * r2 / max(dot(uv - pos2, uv - pos2), 1e-3);
    float influence3 = r3 * r3 / max(dot(uv - pos3, uv - pos3), 1e-3);
    
    // Sum the influences to create a combined field.
    float total_influence = influence1 + influence2 + influence3;

    // Blend the metaball colors based on their relative influence at this pixel.
    vec3 mixed_color = (influence1 * color1 + influence2 * color2 + influence3 * color3) / total_influence;

    // Now, blend the final metaball color with the soft base color.
    // The smoothstep creates the soft, volumetric shape of the combined metaballs.
    // The first parameter (0.75) is the threshold that defines the "surface" of the blobs.
    float blend_factor = smoothstep(0.75, 1.5, total_influence);
    
    vec3 final_rgb = mix(base_color, mixed_color, blend_factor);
    // Subtle contrast & brightness lift to avoid flat look.
    final_rgb = mix(vec3(0.5), final_rgb, 1.05);
    final_rgb *= u_globalBrightness;

    // Final saturation adjustment
    float luma = dot(final_rgb, vec3(0.299, 0.587, 0.114));
    vec3 desaturatedColor = vec3(luma);
    final_rgb = mix(desaturatedColor, final_rgb, u_saturation);

    fragColor = vec4(final_rgb, 1.0);
}
`; 