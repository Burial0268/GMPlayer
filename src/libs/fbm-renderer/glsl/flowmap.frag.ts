export const fragmentShader = `#version 300 es

// This shader uses Fractional Brownian Motion (fBM) noise combined with
// Domain Wrapping to create a seamless, infinitely scrolling distortion field.

precision highp float;
uniform float time;

in vec2 v_texCoord;
out vec4 fragColor;

// A simple hash function to generate pseudo-random numbers.
float hash(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

// 2D noise function based on the hash.
vec2 noise(vec2 p) {
    return vec2(hash(p), hash(p + vec2(13.3, 7.7)));
}

// Fractional Brownian Motion (fBM) function.
// This function layers multiple octaves of noise to create a more detailed,
// natural-looking pattern.
vec2 fbm(vec2 p) {
    vec2 v = vec2(0.0);
    float a = 0.5;
    mat2 rot = mat2(cos(0.5), sin(0.5), -sin(0.5), cos(0.5));
    for (int i = 0; i < 5; i++) {
        v += a * noise(p);
        p = rot * p * 2.0;
        a *= 0.5;
    }
    return v;
}


void main() {
    // Scale the coordinates and add time to create animation.
    vec2 p = v_texCoord * 3.0 + time * 0.1;

    // Separate the coordinates into integer and fractional parts for wrapping.
    vec2 i = floor(p);
    vec2 f = fract(p);

    // Smoothstep the fractional part for smoother interpolation.
    vec2 u = f * f * (3.0 - 2.0 * f);

    // Sample the fbm noise at the four corners of the grid cell.
    vec2 a = fbm(i + vec2(0.0, 0.0));
    vec2 b = fbm(i + vec2(1.0, 0.0));
    vec2 c = fbm(i + vec2(0.0, 1.0));
    vec2 d = fbm(i + vec2(1.0, 1.0));

    // Bilinearly interpolate the results to get the final, smooth motion vector.
    vec2 motion = mix(mix(a, b, u.x), mix(c, d, u.x), u.y);

    // The final vector is centered around 0.5, so we subtract 0.5 to make it
    // a directional vector ranging from -0.5 to 0.5.
    // Pack back into 0..1 range to avoid precision issues when sampling as a texture.
    vec2 packedMotion = motion * 0.5 + 0.5;
    fragColor = vec4(packedMotion, 0.0, 1.0);
}
`;
