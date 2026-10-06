// Stretches the low-resolution game image over the window: each game pixel
// becomes a hard-edged block, colours snap to a reduced palette, and a 4x4
// ordered (Bayer) dither pattern breaks up the colour bands.

#import bevy_ui::ui_vertex_output::UiVertexOutput

// x = colour levels per channel, y = dither strength (0..1).
@group(1) @binding(0) var<uniform> settings: vec4<f32>;
@group(1) @binding(1) var screen: texture_2d<f32>;
@group(1) @binding(2) var screen_sampler: sampler;

// Classic 4x4 Bayer matrix, as thresholds in 0..1.
fn bayer4(p: vec2<u32>) -> f32 {
    let m = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    return (m[(p.y % 4u) * 4u + (p.x % 4u)] + 0.5) / 16.0;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3(0.0)), vec3(1.0 / 2.2));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3(0.0)), vec3(2.2));
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // Read the exact game pixel under this screen pixel: no blending between
    // neighbours, so pixels stay crisp blocks.
    let dims = textureDimensions(screen);
    let px = min(vec2<u32>(in.uv * vec2<f32>(dims)), dims - vec2(1u));
    // The image is sRGB, so this arrives linear. Quantise in sRGB (perceptual)
    // space so the steps look even.
    var c = linear_to_srgb(textureLoad(screen, px, 0).rgb);

    let levels = max(settings.x, 2.0);
    let step = 1.0 / (levels - 1.0);
    let threshold = (bayer4(px) - 0.5) * settings.y;
    c = floor(c / step + 0.5 + threshold) * step;

    return vec4(srgb_to_linear(clamp(c, vec3(0.0), vec3(1.0))), 1.0);
}
