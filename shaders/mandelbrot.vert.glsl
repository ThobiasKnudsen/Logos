#version 450
layout(set = 1, binding = 0, std140) uniform VertexUBO {
    vec2 resolution;
    vec2 padding;
} ubo;
layout(location = 0) in vec4 in_rect; // x, y, w, h
layout(location = 1) in float in_rotation;
layout(location = 2) in float in_corner_radius;
layout(location = 3) in uint in_color_bytes;  // Packed RGBA
layout(location = 4) in uint in_tex_index;
layout(location = 5) in vec4 in_tex_rect; // x, y, w, h for uv
layout(location = 0) out vec2 frag_uv;
layout(location = 1) out vec4 frag_color;
layout(location = 2) out float frag_corner_radius;
layout(location = 3) out uint frag_tex_index;
layout(location = 4) out vec2 frag_tex_coord;
mat2 rotation_matrix(float angle) {
    float c = cos(angle);
    float s = sin(angle);
    return mat2(c, -s, s, c);
}
vec2 get_quad_vertex(int vertex_id) {
    switch (vertex_id) {
        case 0: return vec2(-0.5, -0.5);
        case 1: return vec2( 0.5, -0.5);
        case 2: return vec2(-0.5, 0.5);
        case 3: return vec2( 0.5, 0.5);
    }
    return vec2(0.0);
}
vec4 unpackColor(uint color) {
    float r = float(color & 0xFFu) / 255.0;
    float g = float((color >> 8) & 0xFFu) / 255.0;
    float b = float((color >> 16) & 0xFFu) / 255.0;
    float a = float((color >> 24) & 0xFFu) / 255.0;
    return vec4(r, g, b, a);
}
void main() {
    vec2 local_pos = get_quad_vertex(gl_VertexIndex);
    vec2 scaled_pos = local_pos * vec2(in_rect.z, in_rect.w);
    vec2 rotated_pos = rotation_matrix(in_rotation) * scaled_pos;
    vec2 world_pos = vec2(in_rect.x, in_rect.y) + rotated_pos;

    gl_Position = vec4((world_pos / ubo.resolution) * 2.0 - 1.0, 0.0, 1.0);
    gl_Position.y = -gl_Position.y; // Flip Y for Vulkan/SDL

    frag_uv = (world_pos / ubo.resolution);

    frag_color = vec4(in_color_bytes) / 255.0;
    frag_corner_radius = in_corner_radius;
    frag_tex_index = in_tex_index;

    vec2 tex_local = (local_pos + 0.5); // 0 to 1
    frag_tex_coord = vec2(in_tex_rect.x, in_tex_rect.y) + tex_local * vec2(in_tex_rect.z, in_tex_rect.w);
}