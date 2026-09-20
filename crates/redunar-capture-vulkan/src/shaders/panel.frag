#version 450

layout(location = 0) out vec4 color;

layout(push_constant) uniform PanelPushConstants {
    vec4 bounds;
    float radius;
    uint palette;
} panel;

vec3 panel_color(uint palette) {
    switch (palette) {
        case 1: return vec3(0.027, 0.067, 0.086);
        case 2: return vec3(0.071, 0.051, 0.031);
        case 3: return vec3(0.027, 0.067, 0.047);
        case 4: return vec3(0.035, 0.035, 0.035);
        case 5: return vec3(0.055, 0.039, 0.078);
        case 6: return vec3(0.071, 0.063, 0.024);
        case 7: return vec3(0.078, 0.035, 0.063);
        default: return vec3(0.035, 0.035, 0.035);
    }
}

vec3 border_color(uint palette) {
    switch (palette) {
        case 1: return vec3(0.16, 0.25, 0.28);
        case 2: return vec3(0.27, 0.22, 0.17);
        case 3: return vec3(0.16, 0.26, 0.20);
        case 4: return vec3(0.22, 0.22, 0.22);
        case 5: return vec3(0.24, 0.20, 0.29);
        case 6: return vec3(0.28, 0.26, 0.16);
        case 7: return vec3(0.29, 0.19, 0.23);
        default: return vec3(0.19, 0.19, 0.21);
    }
}

void main() {
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
            color = vec4(border_color(panel.palette), 1.0);
            return;
        }
    }
    color = vec4(panel_color(panel.palette), 1.0);
}
