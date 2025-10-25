
// RESERVED START
axis_1.val
axis_1.min
axis_1.max
axis_1.res
axis_2.val
axis_2.min
axis_2.max
axis_2.res
axis_3.val
axis_3.min
axis_3.max
axis_3.res
time.s
time.ms
time.us
red 
green
blue
var(min max res)
// RESERVED END

BASE_ITER = 16 
BAILOUT = 4.0  
MAX_ITER_CAP = 512
INITIAL_ZOOM = 0.2
ROOT_SAMPLES = 3  

mandelbrot(x, y):
    i32 num_samples = ROOT_SAMPLES * ROOT_SAMPLES
    f32 avg_r = 0.0, avg_g = 0.0, avg_b = 0.0
    f32 width_x = max_x - min_x
    f32 width_y = max_y - min_y
    f32 effective_zoom = 1.0 / width_y
    for (i32 s = 0; s < num_samples; s++)
        f32 jitter_x = (f32(s % ROOT_SAMPLES) + f32((s * 7 + 3) % 100) / 99.0) / f32(ROOT_SAMPLES) - 0.5
        f32 input_x = x + jitter_x / res.x
        f32 c_x = min_x + input_x * width_x
        f32 jitter_y = (f32(s / ROOT_SAMPLES) + f32((s * 13 + 5) % 100) / 99.0) / f32(ROOT_SAMPLES) - 0.5
        f32 input_y = y + jitter_y / res.y
        f32 c_y = min_y + input_y * width_y
        f32 z_x = 0.0, z_y = 0.0, sq = 0.0
        i32 max_iter = min(BASE_ITER + (40.0 * log(effective_zoom / INITIAL_ZOOM + 1.0)), MAX_ITER_CAP)
        i32 iter = 0
        while (iter < max_iter && (sq = z_x * z_x + z_y * z_y) < BAILOUT):
            f32 zy2 = z_y * z_y
            z_y = 2.0 * z_x * z_y + c_y
            z_x = z_x * z_x - zy2 + c_x
            iter++
        if (iter < max_iter):
            f32 uhh = (f32(iter)+1.0-log(0.5*log(sq)/log(2.0))/log(2.0))
            uhh2(val):
                (0.9+0.1*cos(0.05*uhh+0.5*time_s))*((1.0-(0.8+0.2*sin(0.1*uhh+time_s)))+(0.8+0.2*sin(0.1*uhh+time_s))*clamp(abs(fract(fract(0.05*uhh+0.3*time_s)+val)*6.0-3.0)-1.0,0.0,1.0));
            avg_r += uhh2(1.0/2.0)
            avg_g += uhh2(1.0/3.0)
            avg_b += uhh2(1.0/4.0)
    f32 a = sin(time_s * 0.3) * 0.5 + 0.5
    f32 r = avg_r / num_samples
    f32 g = avg_g / num_samples
    f32 b = avg_b / num_samples

    red == r*(1-a) + g*x && green == g*(1-a) + r*x && blue == b*(1-a) + b*a

mandelbrot(axis_1.val, axis_2.val)

x²+y²==9 and ((x>y² or x<y) and x+y!=3)

f32 d_x = (x.max-x.min)/(2.0*x.res)
f32 d_y = (y.max-y.min)/(2.0*y.res)
b8 v1_1 = (x-dx)²+(y-dy)²>9;
b8 v1_2 = (x-dx)²+(y+dy)²>9;
b8 v1_3 = (x+dx)²+(y-dy)²>9;
b8 v1_4 = (x+dx)²+(y+dy)²>9;
b8 v1 = !(v1_1==v1_2 && v1_2==a3 && v1_3==v1_4);
b8 v2_1 = (x-dx)>(y-dy)²;
b8 v2_2 = (x-dx)>(y+dy)²;
b8 v2_3 = (x+dx)>(y-dy)²;
b8 v2_4 = (x+dx)>(y+dy)²;
b8 v2_2 = v2_1==true && v2_1==v2_2 && v2_2==v2_3 && v2_3==v2_4;
b8 v3_1 = (x-dx)<(y-dy);
b8 v3_2 = (x-dx)<(y+dy);
b8 v3_3 = (x+dx)<(y-dy);
b8 v3_4 = (x+dx)<(y+dy);
b8 v3 = v3_1==true && v3_1==v3_2 && v3_2==b3 && v3_3==v3_4;
b8 v4_1 = (x-dx)+(y-dy)>3;
b8 v4_2 = (x-dx)+(y+dy)>3;
b8 v4_3 = (x+dx)+(y-dy)>3;
b8 v4_4 = (x+dx)+(y+dy)>3;
b8 v4 = v4_1==v4_2 && v4_2==v4_3 && v4_3==v4_4;
v1 && ((v2 || v3) && v4)