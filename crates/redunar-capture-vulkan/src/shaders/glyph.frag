#version 450

layout(location = 0) in vec2 glyph_uv;
layout(location = 0) out vec4 color;

layout(push_constant) uniform GlyphRaster {
    uint word0;
    uint word1;
    uint word2;
    uint word3;
    uint word4;
    uint word5;
    uint word6;
    uint word7;
    uint word8;
    uint word9;
    uint word10;
    uint word11;
    uint word12;
    uint word13;
    uint word14;
    uint word15;
    uint word16;
    uint word17;
    uint word18;
    uint word19;
    uint word20;
    uint word21;
    uint word22;
    uint word23;
    uint word24;
    uint word25;
    uint word26;
} glyph;

uint glyph_word(uint index) {
    // Keep every push-constant member explicit. Some game drivers have
    // rendered dynamically indexed push-constant arrays with the wrong
    // element stride even though pipeline creation succeeds.
    return index == 0u ? glyph.word0
        : index == 1u ? glyph.word1
        : index == 2u ? glyph.word2
        : index == 3u ? glyph.word3
        : index == 4u ? glyph.word4
        : index == 5u ? glyph.word5
        : index == 6u ? glyph.word6
        : index == 7u ? glyph.word7
        : index == 8u ? glyph.word8
        : index == 9u ? glyph.word9
        : index == 10u ? glyph.word10
        : index == 11u ? glyph.word11
        : index == 12u ? glyph.word12
        : index == 13u ? glyph.word13
        : index == 14u ? glyph.word14
        : index == 15u ? glyph.word15
        : index == 16u ? glyph.word16
        : index == 17u ? glyph.word17
        : index == 18u ? glyph.word18
        : index == 19u ? glyph.word19
        : index == 20u ? glyph.word20
        : index == 21u ? glyph.word21
        : index == 22u ? glyph.word22
        : index == 23u ? glyph.word23
        : index == 24u ? glyph.word24
        : index == 25u ? glyph.word25
        : glyph.word26;
}

float glyph_sample(ivec2 pixel) {
    if (pixel.x < 0 || pixel.x >= 18 || pixel.y < 0 || pixel.y >= 24) {
        return 0.0;
    }
    uint index = uint(pixel.y) * 18u + uint(pixel.x);
    uint word_index = index / 16u;
    uint shift = (index % 16u) * 2u;
    return float((glyph_word(word_index) >> shift) & 3u) / 3.0;
}

void main() {
    // The source is authored at twice the logical overlay resolution. Manual
    // reconstruction preserves that genuine detail at fractional scales while
    // keeping the injected pipeline descriptor-free.
    vec2 source = glyph_uv * vec2(18.0, 24.0) - vec2(0.5);
    ivec2 base = ivec2(floor(source));
    vec2 fraction = fract(source);
    float upper = mix(
        glyph_sample(base),
        glyph_sample(base + ivec2(1, 0)),
        fraction.x
    );
    float lower = mix(
        glyph_sample(base + ivec2(0, 1)),
        glyph_sample(base + ivec2(1, 1)),
        fraction.x
    );
    float coverage = mix(upper, lower, fraction.y);
    // Coverage is stored in a compact two-bit raster. A mild display-gamma
    // correction restores the weight of the source Noto strokes without
    // thresholding away their antialiased edges at fractional overlay scales.
    coverage = pow(coverage, 0.72);
    if (coverage <= 0.01) {
        discard;
    }
    color = vec4(vec3(coverage), coverage);
}
