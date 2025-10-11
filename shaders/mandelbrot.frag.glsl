#version 450
layout(set = 3, binding = 0, std140) uniform FragmentUBO {
    float time;
} fubo;
layout(location = 0) in vec2 frag_uv;
layout(location = 1) in vec4 frag_color;
layout(location = 2) in float frag_corner_radius;
layout(location = 3) flat in uint frag_tex_index;
layout(location = 4) in vec2 frag_tex_coord;
layout(location = 0) out vec4 out_color;

#define BASE_ITER 10
#define MANDELBROT_INITIAL_ZOOM 0.2LF
#define MANDELBROT_INITIAL_CENTER dvec2(-0.7LF, 0.0LF)
#define MANDELBROT_TARGET_CENTER dvec2(-0.74364501LF, 0.131827LF)
#define ZOOM_SPEED 0.7LF // Reduced speed for longer zoom duration
#define BAILOUT 4.0LF

vec3 mandelbrot_color(double mu) {
    float t = float(mu * 5.0LF); // Increased frequency for more colorful bands
    vec3 phase = vec3(0.0, 0.6, 1.0);
    float base = 3.0 + t * 0.15;
    vec3 arg = vec3(base) + phase;
    vec3 res = 0.5 + 0.5 * cos(arg);
    return res;
}

void main() {
    // Use double precision for deeper zoom without precision artifacts
    double dtime = double(fubo.time);
    float fdtime = float(dtime);

    // Compute dynamic zoom and center for animated zoom into a beautiful region (Seahorse Valley)
    double zoom = MANDELBROT_INITIAL_ZOOM * double(exp(fdtime * float(ZOOM_SPEED)));
    dvec2 center = MANDELBROT_INITIAL_CENTER + (MANDELBROT_TARGET_CENTER - MANDELBROT_INITIAL_CENTER) * (1.0LF - MANDELBROT_INITIAL_ZOOM / zoom);

    // Compute max iterations based on zoom level for deeper detail; more aggressive scaling
    double log_zoom = double(log(float(zoom / MANDELBROT_INITIAL_ZOOM + 1.0LF))); // +1 to avoid log(0)
    int max_iter = BASE_ITER + int(16.0LF * log_zoom); // Increased scaling for deeper iterations

    dvec2 c = center + dvec2(frag_uv - 0.5) / zoom;
    dvec2 z = dvec2(0.0LF);
    int iter = 0;
    for (int i = 0; i < 16384; ++i) { // Increased upper limit to allow deeper iterations
        if (iter >= max_iter) break;
        z = dvec2(z.x * z.x - z.y * z.y, 2.0LF * z.x * z.y) + c;
        if (dot(z, z) > BAILOUT) break;
        ++iter;
    }
    vec3 col;
    if (iter == max_iter) {
        col = vec3(0.0); // Black for points inside the set
    } else {
        // Smooth coloring for beautiful gradients, using double for precision
        double dz2 = dot(z, z);
        float fdz2 = float(dz2);
        float log_bail = log(float(BAILOUT));
        double mu = double(iter) + 1.0LF - double(log(log(fdz2) - log_bail)) / double(log(2.0));
        col = mandelbrot_color(mu);
    }
    // Add some time-based hue shift
    col = mix(col, col.yxz, float(sin(fdtime * 0.3)) * 0.5 + 0.5);
    out_color = vec4(col, 1.0);

    // Optional: if you want to use texture or color, uncomment below
    // if (frag_tex_index > 0u) {
    // out_color = texture(tex_sampler[frag_tex_index - 1u], frag_tex_coord) * frag_color;
    // } else {
    // out_color = frag_color;
    // }
    // // Apply rounded corners if needed (note: ubo.resolution.x requires passing resolution to fragment UBO too)
    // vec2 q = abs(frag_uv * 2.0 - 1.0) - 1.0 + frag_corner_radius / 1000.0; // Approximate, or add resolution to fubo
    // float rounded = min(min(q.x, q.y), 0.0) + length(max(q, 0.0)) - frag_corner_radius / 1000.0;
    // if (rounded > 0.0) discard;
}