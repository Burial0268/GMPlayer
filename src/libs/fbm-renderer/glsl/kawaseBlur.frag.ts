export const fragmentShader = `#version 300 es

precision highp float;

uniform sampler2D u_texture;
uniform int u_level;
uniform vec2 resolution;

in vec2 v_texCoord;

out vec4 fragColor;

vec4 reSample(sampler2D tex, vec2 res, in int d, in vec2 uv)
{
    vec2 step1 = (vec2(d) + 0.5) / res;
    vec4 color = vec4(0.0);
    
    color += texture(tex, uv + step1) / float(4);
    color += texture(tex, uv - step1) / float(4);
    vec2 step2 = step1;
    step2.x = -step2.x;
    color += texture(tex, uv + step2) / float(4);
    color += texture(tex, uv - step2) / float(4);

    return color;
}

void main() {
    fragColor = reSample(u_texture, resolution, u_level, v_texCoord);
}`;
