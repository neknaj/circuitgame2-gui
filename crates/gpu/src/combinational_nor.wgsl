@group(0) @binding(0) var<storage, read_write> wires: array<u32>;
@group(0) @binding(1) var<storage, read> routes: array<vec2<u32>>;
@group(0) @binding(2) var<storage, read> ports: array<u32>;
@group(0) @binding(3) var<storage, read_write> output: array<u32>;
@group(0) @binding(4) var<storage, read> dimensions: vec4<u32>;

@compute @workgroup_size(64)
fn evaluate(@builtin(global_invocation_id) id: vec3<u32>) {
    let word = id.x;
    let words = dimensions.x;
    if word >= words {
        return;
    }

    // 配線番号は入力、ゲート、偽、真の順。各 invocation は自身のワードだけを読み書きする。
    let inputs = dimensions.y;
    let gates = dimensions.z;
    let constant_base = inputs + gates;
    wires[constant_base * words + word] = 0u;
    wires[(constant_base + 1u) * words + word] = 0xffffffffu;
    for (var gate = 0u; gate < gates; gate++) {
        let route = routes[gate];
        let left = wires[route.x * words + word];
        let right = wires[route.y * words + word];
        wires[(inputs + gate) * words + word] = ~(left | right);
    }
    for (var port = 0u; port < dimensions.w; port++) {
        output[port * words + word] = wires[ports[port] * words + word];
    }
}
