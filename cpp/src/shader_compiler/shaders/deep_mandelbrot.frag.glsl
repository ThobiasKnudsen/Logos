#version 450
#extension GL_ARB_gpu_shader_fp64 : enable

layout(set = 3, binding = 0, std140) uniform FragmentUBO {
    float time;
    vec2 resolution;
    vec2 center;
    float zoom;
    float padding;
} fubo;

layout(location = 0) in vec2 frag_uv;

layout(location = 0) out vec4 out_color;

#define BASE_ITER 100 
#define BAILOUT 4.0LF
#define MAX_ITER_CAP 16384
#define INITIAL_ZOOM 0.2f

vec3 hsv2rgb(vec3 c) {
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

vec3 compute_mandelbrot_color(dvec2 c, int max_iter, float time) {
    dvec2 z = dvec2(0.0LF);
    int iter = 0;
    double sq = 0.0LF;
    while (iter < max_iter && (sq = dot(z, z)) < BAILOUT) {
        z = dvec2(z.x * z.x - z.y * z.y, 2.0LF * z.x * z.y) + c;
        iter++;
    }
    if (iter == max_iter) {
        return vec3(0.0); // Black for points inside the set
    }
    // Smooth coloring - use float precision for log to avoid double transcendental issues
    float sq_f = float(sq);
    float log_zn_f = 0.5f * log(sq_f);
    float nu_f = log(log_zn_f / log(2.0f)) / log(2.0f);
    float mu = float(iter) + 1.0f - nu_f;
    // Animated HSV-based color for beauty: cycling hues with saturation and value modulation
    float hue = fract(0.05 * mu + 0.1 * time);
    float sat = 0.8 + 0.2 * sin(0.1 * mu + time);
    float val = 0.9 + 0.1 * cos(0.05 * mu + 0.5 * time);
    return hsv2rgb(vec3(hue, sat, val));
}

// New: Supersampling function taking the root (e.g., 4 for 4x4=16 samples per pixel)
// Easy to call with different roots; handles jittered grid offsets automatically
vec3 supersample(vec2 base_uv, int root_samples, float time, int max_iter, vec2 resolution, vec2 center, float zoom) {
    int num_samples = root_samples * root_samples;
    vec3 avg_col = vec3(0.0f);
    float aspect = resolution.x / resolution.y;
    double zoom_d = double(zoom);
    dvec2 center_d = dvec2(center);
    for (int s = 0; s < num_samples; ++s) {
        // Grid-based with simple jitter: map to [0, root_samples) then fractional offset
        int grid_x = s % root_samples;
        int grid_y = s / root_samples;
        // Jitter: pseudo-random frac offset in cell (using s-based hash for variety)
        float frac_x = (float((s * 7 + 3) % 100) / 99.0f); // 0 to ~1
        float frac_y = (float((s * 13 + 5) % 100) / 99.0f);
        vec2 grid_offset = vec2(float(grid_x) + frac_x, float(grid_y) + frac_y) / float(root_samples) - 0.5f;
        vec2 pixel_offset = grid_offset / resolution;
        vec2 sample_uv = base_uv + pixel_offset;

        dvec2 c = center_d + dvec2(sample_uv - 0.5f) * dvec2(aspect, 1.0) / zoom_d;
        avg_col += compute_mandelbrot_color(c, max_iter, time);
    }
    return avg_col / float(num_samples);
}

void main() {
    // Use double precision for deeper zooms
    float time = fubo.time;
    vec2 center = fubo.center;
    float zoom = fubo.zoom;

    // Compute max iterations based on zoom level for deeper detail; increased scaling for deeper zooms
    float log_zoom = log(zoom / INITIAL_ZOOM + 1.0f); // +1 to avoid log(0)
    int uncapped_iter = BASE_ITER + int(500.0f * log_zoom); // Further increased multiplier for much deeper detail
    int max_iter = min(uncapped_iter, MAX_ITER_CAP); // Increased cap to 16384 for extreme zooms

    vec3 col;
    // Safeguard: If resolution invalid (e.g., (0,0) from UBO mismatch), fall back to single sample to avoid NaN
    if (fubo.resolution.x <= 0.0f || fubo.resolution.y <= 0.0f) {
        col = vec3(1.0f, 0.0f, 0.0f);
    } else {
        // Use the new supersample function: change ROOT_SAMPLES below to adjust points per pixel (e.g., 4=16, 3=9, 2=4, 1=single)
        const int ROOT_SAMPLES = 2;  // 4 samples per pixel for balance of quality and performance at deep zooms
        col = supersample(frag_uv, ROOT_SAMPLES, time, max_iter, fubo.resolution, center, zoom);
        // Add some time-based hue shift
        col = mix(col, col.yxz, sin(time * 0.3f) * 0.5f + 0.5f);
    }
    out_color = vec4(col, 1.0);
}