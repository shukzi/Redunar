#version 450

layout(location = 0) out vec4 color;

layout(push_constant) uniform PanelPushConstants {
    vec4 bounds;
    float radius;
    uint palette;
} panel;

vec3 panel_color(uint palette) {
    switch (palette) {
        // The Replay menu keeps its own near-black surface independent of the
        // eight metric palettes.
        case 8: return vec3(0.003, 0.003, 0.003);
        case 1: return vec3(0.002, 0.006, 0.008);
        case 2: return vec3(0.006, 0.004, 0.002);
        case 3: return vec3(0.002, 0.006, 0.004);
        case 4: return vec3(0.003, 0.003, 0.003);
        case 5: return vec3(0.004, 0.003, 0.007);
        case 6: return vec3(0.006, 0.005, 0.002);
        case 7: return vec3(0.007, 0.003, 0.005);
        default: return vec3(0.003, 0.003, 0.003);
    }
}

vec3 border_color(uint palette) {
    switch (palette) {
        case 8: return vec3(0.019, 0.022, 0.030);
        case 1: return vec3(0.022, 0.051, 0.063);
        case 2: return vec3(0.060, 0.040, 0.024);
        case 3: return vec3(0.022, 0.054, 0.033);
        case 4: return vec3(0.040, 0.040, 0.040);
        case 5: return vec3(0.047, 0.033, 0.068);
        case 6: return vec3(0.063, 0.054, 0.022);
        case 7: return vec3(0.068, 0.030, 0.044);
        default: return vec3(0.030, 0.030, 0.037);
    }
}

void main() {
    uint palette = panel.palette % 9u;
    bool srgb_attachment = panel.palette >= 9u;
    if (panel.radius > 0.0) {
        vec2 local = gl_FragCoord.xy - panel.bounds.xy;
        vec2 half_size = panel.bounds.zw * 0.5;
        vec2 distance_to_edge = abs(local - half_size) - (half_size - vec2(panel.radius));
        float rounded_distance = length(max(distance_to_edge, vec2(0.0)))
            + min(max(distance_to_edge.x, distance_to_edge.y), 0.0)
            - panel.radius;
        if (rounded_distance > 0.0) {
            discard;
        }
        if (rounded_distance > -1.0) {
            vec3 rgb = border_color(palette);
            color = vec4(srgb_attachment ? rgb : sqrt(rgb), 1.0);
            return;
        }
    }
    vec3 rgb = panel_color(palette);
    color = vec4(srgb_attachment ? rgb : sqrt(rgb), 1.0);
}
