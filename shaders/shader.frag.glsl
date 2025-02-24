#version 450

layout(location = 0) in vec4        fragColor;
layout(location = 1) in vec2        fragTexCoord;
layout(location = 2) flat in uint   fragTexIndex;
layout(location = 3) in float       fragCornerRadiusWidth;
layout(location = 4) in float       fragCornerRadiusHeight;

layout(location = 0) out vec4 outColor;

layout(set = 2, binding = 0) uniform sampler2D texture_0;
layout(set = 2, binding = 1) uniform sampler2D texture_1;
layout(set = 2, binding = 2) uniform sampler2D texture_2;
layout(set = 2, binding = 3) uniform sampler2D texture_3;
layout(set = 2, binding = 4) uniform sampler2D texture_4;
layout(set = 2, binding = 5) uniform sampler2D texture_5;
layout(set = 2, binding = 6) uniform sampler2D texture_6;
layout(set = 2, binding = 7) uniform sampler2D texture_7;

void main() {
    vec4 textureColor = vec4(0.0);
    if (fragTexIndex == 1) {textureColor = texture(texture_0, fragTexCoord);} 
    else if (fragTexIndex == 2) {textureColor = texture(texture_1, fragTexCoord);} 
    else if (fragTexIndex == 3) {textureColor = texture(texture_2, fragTexCoord);} 
    else if (fragTexIndex == 4) {textureColor = texture(texture_3, fragTexCoord);} 
    else if (fragTexIndex == 5) {textureColor = texture(texture_4, fragTexCoord);} 
    else if (fragTexIndex == 6) {textureColor = texture(texture_5, fragTexCoord);} 
    else if (fragTexIndex == 7) {textureColor = texture(texture_6, fragTexCoord);} 
    else if (fragTexIndex == 8) {textureColor = texture(texture_7, fragTexCoord);}
    
    vec4 color = (fragTexIndex == 0 || fragTexIndex >= 9)
        ? fragColor
        : fragColor * textureColor;

    float x = fragTexCoord.x;
    x = x > 0.5 ? 1.0 - x : x;
    float y = fragTexCoord.y;
    y = y > 0.5 ? 1.0 - y : y;
    float edge = 0.003;

    if (fragCornerRadiusWidth < 0.0001 || fragCornerRadiusHeight < 0.0001 ||
        x >= fragCornerRadiusWidth + edge || y >= fragCornerRadiusHeight + edge) {
        outColor = color;
        return;
    }
    x = x - fragCornerRadiusWidth * 0.98;
    y = y - fragCornerRadiusHeight * 0.98;
    float a = (pow(x / fragCornerRadiusWidth, 2)) + (pow(y / fragCornerRadiusHeight, 2));
    if (a < 1.0) {
        if (a > 0.9) {
            outColor = vec4(color.rgb, color.a * (1.0 - a) * 10.0);
            return;
        }
        outColor = color;
        return;
    }
    outColor = vec4(0.0);
}
