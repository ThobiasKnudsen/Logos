#version 450

layout(set = 3, binding = 0, std140) uniform FragmentUBO {
    float time;
    float padding;
    vec2 res;
    float min_x;
    float max_x;
    float min_y;
    float max_y;
} fubo;

layout(location = 0) in vec2 frag_uv;

layout(location = 0) out vec4 out_color;

#define BASE_ITER 16 
#define BAILOUT 4.0f
#define MAX_ITER_CAP 512
#define INITIAL_ZOOM 0.2f
#define ROOT_SAMPLES 3

vec3 supersample_mandelbrot(float base_uv_x, float base_uv_y) {
    int num_samples = ROOT_SAMPLES * ROOT_SAMPLES;
    float avg_r = 0.0f, avg_g = 0.0f, avg_b = 0.0f;
    float width_x = fubo.max_x - fubo.min_x;
    float width_y = fubo.max_y - fubo.min_y;
    float effective_zoom = 1.0f / width_y;
    for (int s = 0; s < num_samples; ++s) {
        float jitter_x = (float(s % ROOT_SAMPLES) + float((s * 7 + 3) % 100) / 99.0f) / float(ROOT_SAMPLES) - 0.5f;
        float input_x = base_uv_x + jitter_x / fubo.res.x;
        float c_x = fubo.min_x + input_x * width_x;
        float jitter_y = (float(s / ROOT_SAMPLES) + float((s * 13 + 5) % 100) / 99.0f) / float(ROOT_SAMPLES) - 0.5f;
        float input_y = base_uv_y + jitter_y / fubo.res.y;
        float c_y = fubo.min_y + input_y * width_y;
        float z_x = 0.0f, z_y = 0.0f, sq = 0.0f;
        int max_iter = min(BASE_ITER + int(40.0f * log(effective_zoom / INITIAL_ZOOM + 1.0f)), MAX_ITER_CAP); 
        int iter = 0;
        while (iter < max_iter && (sq = z_x * z_x + z_y * z_y) < BAILOUT) {
            float zy2 = z_y * z_y;
            z_y = 2.0f * z_x * z_y + c_y;
            z_x = z_x * z_x - zy2 + c_x;
            iter++;
        }
        if (iter < max_iter) {
            float uhh = (float(iter) + 1.0f - log(0.5f * log(sq) / log(2.0f)) / log(2.0f));
            avg_r += (0.9f + 0.1f * cos(0.05f * uhh + 0.5f * fubo.time)) * ((1.0f - (0.8f + 0.2f * sin(0.1f * uhh + fubo.time))) + (0.8f + 0.2f * sin(0.1f * uhh + fubo.time)) * clamp(abs(fract(fract(0.05f * uhh + 0.3f * fubo.time) + 1.0f / 2.0f) * 6.0f - 3.0f) - 1.0f, 0.0f, 1.0f));
            avg_g += (0.9f + 0.1f * cos(0.05f * uhh + 0.5f * fubo.time)) * ((1.0f - (0.8f + 0.2f * sin(0.1f * uhh + fubo.time))) + (0.8f + 0.2f * sin(0.1f * uhh + fubo.time)) * clamp(abs(fract(fract(0.05f * uhh + 0.2f * fubo.time) + 1.0f / 3.0f) * 6.0f - 3.0f) - 1.0f, 0.0f, 1.0f));
            avg_b += (0.9f + 0.1f * cos(0.05f * uhh + 0.5f * fubo.time)) * ((1.0f - (0.8f + 0.2f * sin(0.1f * uhh + fubo.time))) + (0.8f + 0.2f * sin(0.1f * uhh + fubo.time)) * clamp(abs(fract(fract(0.05f * uhh + 0.1f * fubo.time) + 1.0f / 5.0f) * 6.0f - 3.0f) - 1.0f, 0.0f, 1.0f));
        }
    }
    vec3 col = vec3(avg_r / float(num_samples), avg_g / float(num_samples), avg_b / float(num_samples));
    col = mix(col, col.yxz, sin(fubo.time * 0.3f) * 0.5f + 0.5f);
    return col;
}

void main() {
    vec3 col;
    // Safeguard: If res invalid (e.g., (0,0) from UBO mismatch), fall back to single sample to avoid NaN
    if (fubo.res.x <= 0.0f || fubo.res.y <= 0.0f) {
        col = vec3(1.0f, 0.0f, 0.0f);
    } else {
        //col = supersample(frag_uv, ROOT_SAMPLES, time, max_iter, fubo.res, center, zoom);
        col = supersample_mandelbrot(frag_uv.x, frag_uv.y);
    }
    out_color = vec4(col, 1.0);
}