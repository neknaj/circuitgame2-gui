@group(0) @binding(0) var<storage, read> old_gates: array<u32>;
@group(0) @binding(1) var<storage, read_write> next_gates: array<u32>;
@group(0) @binding(2) var<storage, read> links: array<vec2<u32>>;
@group(0) @binding(3) var<storage, read> frames: array<u32>;
@group(0) @binding(4) var<storage, read> ports: array<u32>;
@group(0) @binding(5) var<storage, read_write> trace: array<u32>;
@group(0) @binding(6) var<storage, read> dimensions: array<u32>;

fn wire_value(slot: u32, word: u32, after: bool) -> u32 {
    let words = dimensions[0];
    let input_count = dimensions[1];
    let gate_count = dimensions[2];
    if (slot < input_count) {
        return frames[(dimensions[4] * input_count + slot) * words + word];
    }
    if (slot < input_count + gate_count) {
        let at = (slot - input_count) * words + word;
        if (after) {
            return next_gates[at];
        }
        return old_gates[at];
    }
    return select(0u, 0xffffffffu, slot == input_count + gate_count + 1u);
}

@compute @workgroup_size(64)
fn advance(@builtin(global_invocation_id) id: vec3<u32>) {
    let words = dimensions[0];
    if (id.x < dimensions[2] * words) {
        let wires = links[id.x / words];
        let word = id.x % words;
        next_gates[id.x] = ~(wire_value(wires.x, word, false) | wire_value(wires.y, word, false));
    }
}

@compute @workgroup_size(64)
fn observe(@builtin(global_invocation_id) id: vec3<u32>) {
    let words = dimensions[0];
    let size = dimensions[3] * words;
    if (id.x < size) {
        trace[dimensions[4] * size + id.x] = wire_value(ports[id.x / words], id.x % words, true);
    }
}
