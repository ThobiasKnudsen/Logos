#version 450

layout(set = 3, binding = 0, std140) uniform FragmentUBO {
    float time;
    vec2 resolution;
    vec2 center;
    float zoom;
    float padding;
} fubo;

layout(location = 0) in vec2 frag_uv;

layout(location = 0) out vec4 out_color;

vec3 hsv2rgb(vec3 c) {
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

// Define your function here (example: animated sine wave, with x flipped to correct axis orientation)
// Adjust the expression inside to change the function being integrated
float my_function(float x) {
    return x -x*x;
    // Examples:
    // return - (x * x - 1.0);  // Flipped parabola
    // return exp(-x * x * 0.5) * cos(-x * 5.0 + fubo.time);  // Flipped Gaussian modulated cosine
    // return abs(sin(-x * 2.0 + fubo.time));  // Flipped absolute value
}

// Numerical trapezoidal integration from left_x to x
float compute_integral(float x, float left_x, int steps) {
    if (x <= left_x) {
        return 0.0;
    }
    float h = (x - left_x) / float(steps);
    float sum = 0.0;
    for (int i = 0; i < steps; ++i) {
        float t1 = left_x + float(i) * h;
        float t2 = left_x + float(i + 1) * h;
        sum += (my_function(t1) + my_function(t2)) * 0.5 * h;
    }
    return sum;
}

void main() {
    float time = fubo.time;
    vec2 center = fubo.center;
    float zoom = fubo.zoom;

    vec3 col = vec3(0.0);  // Black background

    // Safeguard for invalid resolution
    if (fubo.resolution.x <= 0.0f || fubo.resolution.y <= 0.0f) {
        col = vec3(1.0f, 0.0f, 0.0f);
    } else {
        float aspect = fubo.resolution.x / fubo.resolution.y;
        float left_x = center.x - 0.5 * aspect / zoom;
        
        // Pixel size in world coordinates
        vec2 pixel_size = vec2(aspect / (zoom * fubo.resolution.x), 1.0 / (zoom * fubo.resolution.y));
        
        vec2 world_pos = center + (frag_uv - 0.5f) * vec2(aspect, 1.0) / zoom;
        
        // Straddle test with 4 sub-pixel samples for anti-aliased line
        const int INTEGRAL_STEPS = 32;  // Adjustable: higher for more accurate integral, but slower
        int above_count = 0;
        vec2 offsets[4] = {vec2(-0.5f, -0.5f), vec2(0.5f, -0.5f), vec2(-0.5f, 0.5f), vec2(0.5f, 0.5f)};
        for (int i = 0; i < 4; ++i) {
            vec2 sub_pos = world_pos + offsets[i] * pixel_size;
            float F = compute_integral(sub_pos.x, left_x, INTEGRAL_STEPS);
            bool is_above = (sub_pos.y > F);
            if (is_above) {
                above_count++;
            }
        }
        
        float frac_above = float(above_count) / 4.0f;
        if (frac_above > 0.0f && frac_above < 1.0f) {
            // Smooth coverage using triangle function for anti-aliasing
            float coverage = 1.0f - 2.0f * abs(frac_above - 0.5f);
            coverage = max(0.0f, coverage);
            
            // Line color: animated HSV for visibility
            float x_norm = (world_pos.x - left_x) / (aspect / zoom);  // Normalized x position in view
            float hue = fract(x_norm * 0.5 + time * 0.1);
            vec3 line_col = hsv2rgb(vec3(hue, 0.8, 1.0));
            
            // Apply coverage
            col = line_col * coverage;
        }
        
        // Optional: Add time-based hue shift for extra animation
        col = mix(col, col.yxz, sin(time * 0.3f) * 0.5f + 0.5f);
    }
    out_color = vec4(col, 1.0);
}