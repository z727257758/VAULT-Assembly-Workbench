struct Camera { view_projection: mat4x4<f32>, };
struct Object { model: mat4x4<f32>, color: vec4<f32>, text_color: vec4<f32>, bed: vec4<f32>, };
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> object: Object;
struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) normal: vec3<f32>, @location(1) shade: f32, @location(2) bed_mm: vec2<f32>, };
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) shade: f32) -> VertexOut {
 var out: VertexOut;
 out.position=camera.view_projection*object.model*vec4<f32>(position,1.0);
 out.shade=shade;
 out.bed_mm=vec2<f32>(position.x,-position.z);
 out.normal=(object.model*vec4<f32>(normal,0.0)).xyz;
 return out;
}

// Distance to the nearest grid line, measured in screen pixels. Fade each
// axis independently once zoom or perspective would make its lines merge.
fn grid_coverage(mm: vec2<f32>, pixel: vec2<f32>, spacing: f32, half_width: f32) -> f32 {
 let distance = abs(fract(mm / spacing + 0.5) - 0.5) * spacing / pixel;
 let coverage = clamp(vec2<f32>(half_width + 0.5) - distance, vec2<f32>(0.), vec2<f32>(1.));
 let visible = coverage * smoothstep(vec2<f32>(3.), vec2<f32>(7.), vec2<f32>(spacing) / pixel);
 return max(visible.x, visible.y);
}

fn print_surface(in: VertexOut, pixel: vec2<f32>) -> vec3<f32> {
 let base = object.color.rgb;
 if in.normal.y < 0.9 {
   return base * 0.68;
 }
 if in.shade > 1.5 {
   return select(vec3<f32>(0.84,0.85,0.81), vec3<f32>(0.39), base.r > 0.70);
 }
 let mm = in.bed_mm;
 let edges = min(mm, vec2<f32>(object.bed.x) - mm) / pixel;
 let border = min(edges.x, edges.y);
 let inside = clamp(border + 0.5, 0., 1.);
 let minor = grid_coverage(mm, pixel, 10., 0.42);
 let major = grid_coverage(mm, pixel, 50., 0.85);
 let ink = vec3<f32>(object.bed.w);
 var face = mix(base, ink, max(minor * 0.63, major));
 if all(mm <= object.bed.yz) && all(mm >= vec2<f32>(0.)) {
   let pitch = 5.;
   let diagonal = abs(fract((mm.x + mm.y) / pitch + 0.5) - 0.5) * pitch;
   let footprint = pixel.x + pixel.y;
   let hatch = clamp((0.44 - diagonal) / footprint + 0.5, 0., 1.) * smoothstep(1., 3., pitch / footprint);
   face = mix(base * 0.65, ink, hatch * 0.65);
 }
 let rim = (1. - smoothstep(0.2, 1.6, abs(border))) * 0.80;
 return mix(mix(base * 0.79, face, inside), ink, rim);
}

@fragment fn fs_main(in:VertexOut,@builtin(front_facing) front:bool) -> @location(0) vec4<f32> {
 let pixel = max(fwidth(in.bed_mm), vec2<f32>(0.0001));
 if object.bed.x > 0. {
   return vec4<f32>(print_surface(in, pixel), 1.);
 }
 let n=normalize(select(-in.normal,in.normal,front));
 let light=0.32+0.52*max(dot(n,normalize(vec3<f32>(-0.4,0.9,0.6))),0.0)+0.18*max(dot(n,normalize(vec3<f32>(0.7,0.2,-0.6))),0.0);
 if object.text_color.a > 0.0 {
   return vec4<f32>(select(object.color.rgb,object.text_color.rgb,in.shade < 0.9)*light,object.color.a);
 }
 return vec4<f32>(object.color.rgb*light*in.shade,object.color.a);
}
