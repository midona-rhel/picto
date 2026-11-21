// Backdrop blur. The frame is drawn into a texture of its own while a blur is
// on screen; each blur shrinks the region around it into a scratch texture
// (averaging boxes of `downsample` pixels), runs a separable gaussian across
// and down that, then draws the result, saturated and brightened, over the
// frame inside its rounded rectangle. The finished frame is then copied to
// the surface.

// Mirrors `BackdropBlurParams` in backdrop_blur.rs.
struct BlurParams {
    // x, y, width, height of the pixels the composite covers.
    target_rect: vec4<f32>,
    // The frame pixel at the first reduced texel's corner, the downsample
    // factor, and the number of taps used.
    source_downsample_taps: vec4<f32>,
    // The reduced region's size, then the scratch texture's.
    reduced_texture: vec4<f32>,
    // The frame's size, then the gaussian's direction.
    viewport_direction: vec4<f32>,
    tap_offsets: array<vec4<f32>, 3>,
    tap_weights: array<vec4<f32>, 3>,
    bounds: vec4<f32>,
    // top left, top right, bottom right, bottom left.
    corner_radii: vec4<f32>,
    clip: vec4<f32>,
    // saturation, brightness, opacity.
    effect: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: BlurParams;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

struct BlurVarying {
    @builtin(position) position: vec4<f32>,
}

fn unit_vertex(vertex_id: u32) -> vec2<f32> {
    return vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
}

// Covers the whole viewport, which each pass sets to the region it writes.
@vertex
fn vs_blur_pass(@builtin(vertex_index) vertex_id: u32) -> BlurVarying {
    var out = BlurVarying();
    out.position = vec4<f32>(unit_vertex(vertex_id) * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return out;
}

fn tap_offset(index: i32) -> f32 {
    return params.tap_offsets[index / 4][index % 4];
}

fn tap_weight(index: i32) -> f32 {
    return params.tap_weights[index / 4][index % 4];
}

// One reduced texel: the average of its box of frame pixels, read two by two
// with bilinear samples between them.
@fragment
fn fs_blur_downsample(input: BlurVarying) -> @location(0) vec4<f32> {
    let viewport = params.viewport_direction.xy;
    let downsample = params.source_downsample_taps.z;
    let box_origin = params.source_downsample_taps.xy + floor(input.position.xy) * downsample;
    if (downsample <= 1.0) {
        return textureSampleLevel(source, source_sampler, (box_origin + 0.5) / viewport, 0.0);
    }
    let pairs = i32(downsample / 2.0);
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < pairs; y += 1) {
        for (var x = 0; x < pairs; x += 1) {
            let point = box_origin + vec2<f32>(f32(2 * x + 1), f32(2 * y + 1));
            sum += textureSampleLevel(source, source_sampler, point / viewport, 0.0);
        }
    }
    return sum / f32(pairs * pairs);
}

// One direction of the gaussian over the reduced region, which ends at its
// own edge rather than at the scratch texture's.
@fragment
fn fs_blur_gaussian(input: BlurVarying) -> @location(0) vec4<f32> {
    let texture_size = params.reduced_texture.zw;
    let low = vec2<f32>(0.5);
    let high = params.reduced_texture.xy - 0.5;
    let direction = params.viewport_direction.zw;
    let center = input.position.xy;
    var sum = textureSampleLevel(source, source_sampler, center / texture_size, 0.0) * tap_weight(0);
    let taps = i32(params.source_downsample_taps.w);
    for (var i = 1; i < taps; i += 1) {
        let offset = direction * tap_offset(i);
        let weight = tap_weight(i);
        sum += textureSampleLevel(source, source_sampler, clamp(center + offset, low, high) / texture_size, 0.0) * weight;
        sum += textureSampleLevel(source, source_sampler, clamp(center - offset, low, high) / texture_size, 0.0) * weight;
    }
    return sum;
}

@vertex
fn vs_blur_composite(@builtin(vertex_index) vertex_id: u32) -> BlurVarying {
    let position = params.target_rect.xy + unit_vertex(vertex_id) * params.target_rect.zw;
    let viewport = params.viewport_direction.xy;
    var out = BlurVarying();
    out.position = vec4<f32>(position / viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return out;
}

fn rounded_rect_sdf(point: vec2<f32>) -> f32 {
    let half_size = params.bounds.zw / 2.0;
    let center_to_point = point - (params.bounds.xy + half_size);
    var radius: f32;
    if (center_to_point.x < 0.0) {
        radius = select(params.corner_radii.w, params.corner_radii.x, center_to_point.y < 0.0);
    } else {
        radius = select(params.corner_radii.z, params.corner_radii.y, center_to_point.y < 0.0);
    }
    let corner_center_to_point = abs(center_to_point) - half_size + radius;
    let inside = min(0.0, max(corner_center_to_point.x, corner_center_to_point.y));
    return length(max(vec2<f32>(0.0), corner_center_to_point)) + inside - radius;
}

// The blurred backdrop inside the rounded rectangle, saturated and
// brightened as CSS `saturate()` and `brightness()` do, premultiplied, at the
// region's coverage.
@fragment
fn fs_blur_composite(input: BlurVarying) -> @location(0) vec4<f32> {
    let point = input.position.xy;
    let clip_end = params.clip.xy + params.clip.zw;
    if (any(point < params.clip.xy) || any(point >= clip_end)) {
        return vec4<f32>(0.0);
    }
    let coverage = saturate(0.5 - rounded_rect_sdf(point)) * params.effect.z;
    let downsample = params.source_downsample_taps.z;
    let texel = clamp(
        (point - params.source_downsample_taps.xy) / downsample,
        vec2<f32>(0.5),
        params.reduced_texture.xy - 0.5,
    );
    var color = textureSampleLevel(source, source_sampler, texel / params.reduced_texture.zw, 0.0);
    let luma = dot(color.rgb, vec3<f32>(0.213, 0.715, 0.072));
    let rgb = mix(vec3<f32>(luma), color.rgb, params.effect.x) * params.effect.y;
    color = vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(color.a)), color.a);
    return color * coverage;
}

// Copies the finished frame to the surface.
@fragment
fn fs_blur_present(input: BlurVarying) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(input.position.xy), 0);
}
