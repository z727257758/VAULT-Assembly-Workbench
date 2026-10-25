struct Camera { view_projection: mat4x4<f32>, };
struct Object { model: mat4x4<f32>, color: vec4<f32>, text_color: vec4<f32>, };
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> object: Object;
struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) normal: vec3<f32>, @location(1) shade: f32, };
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) shade: f32) -> VertexOut {
 var out: VertexOut;
 out.position=camera.view_projection*object.model*vec4<f32>(position,1.0);
 out.shade=shade;
 out.normal=(object.model*vec4<f32>(normal,0.0)).xyz;
 return out;
}
@fragment fn fs_main(in:VertexOut,@builtin(front_facing) front:bool) -> @location(0) vec4<f32> {
 let n=normalize(select(-in.normal,in.normal,front));
 let light=0.32+0.52*max(dot(n,normalize(vec3<f32>(-0.4,0.9,0.6))),0.0)+0.18*max(dot(n,normalize(vec3<f32>(0.7,0.2,-0.6))),0.0);
 if object.text_color.a > 0.0 {
   return vec4<f32>(select(object.color.rgb,object.text_color.rgb,in.shade < 0.9)*light,object.color.a);
 }
 return vec4<f32>(object.color.rgb*light*in.shade,object.color.a);
}
