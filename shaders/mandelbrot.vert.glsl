#version 450

layout(set = 1, binding = 0, std140) uniform VertexUBO {
    vec4 rect;     // x, y, w, h
    vec2 screen_res;
    vec2 padding;
} vubo;

layout(location = 0) out vec2 frag_uv;

vec2 get_quad_vertex(int vertex_id) {
    switch (vertex_id) {
        case 0: return vec2(0.0, 0.0);
        case 1: return vec2(1.0, 0.0);
        case 2: return vec2(0.0, 1.0);
        case 3: return vec2(1.0, 1.0);
    }
    return vec2(0.0);
}

void main() {
    vec2 local_pos = get_quad_vertex(gl_VertexIndex);
    vec2 world_pos = vubo.rect.xy + local_pos * vubo.rect.zw;

    gl_Position = vec4((world_pos / vubo.screen_res) * 2.0 - 1.0, 0.0, 1.0);
    gl_Position.y = -gl_Position.y;

    frag_uv = world_pos / vubo.screen_res;  // Screen-relative UV (0-1 across full screen, but clipped to rect)
}