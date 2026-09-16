> Scope: retain these shader/ABI and asset-generation notes when changing the
> associated source. Dated KMS experiments are not production Replay support;
> the active path is described in [REPLAY.md](../../../../REPLAY.md).

# Redunar overlay shaders

The overlay shaders are original Redunar source. The runtime uses one pipeline
for panel transparency and one fixed-layout antialiased glyph pipeline:

- `panel.vert` emits one buffer-free full-screen triangle using
  `gl_VertexIndex`;
- `panel.frag` emits Redunar's dark panel color;
- dynamic viewport and scissor state constrain the draw to the panel;
- dynamic constant-alpha blending applies the saved 0–100% opacity;
- `glyph.vert` maps one bounded glyph viewport without buffers or descriptors;
- `glyph.frag` reads one genuine 18x24 antialiased raster from 27 explicitly
  named scalar push-constant words. It reconstructs that 2x source at the
  saved fractional overlay scale, so 200% text no longer enlarges 9x12 bitmap
  squares; and
- dynamic constant-color blending selects Redunar accent or body text.

The generated coverage table lives in `redunar-core`. Regenerate it with
`tools/generate-overlay-font.py` and the licensed Noto Sans Mono Regular face;
the installed runtime does not load fonts, allocate an atlas, or compile
shaders.

The checked-in `.spv` files were compiled from these adjacent sources with
Shaderc 2026.1. Shaderc is a development tool only and is not linked into or
required by Redunar at build time or runtime. Regenerate both binaries with a
current Vulkan SDK `glslc` before changing shader source:

```bash
glslc --target-env=vulkan1.0 -fshader-stage=vertex panel.vert -o panel.vert.spv
glslc --target-env=vulkan1.0 -fshader-stage=fragment panel.frag -o panel.frag.spv
glslc --target-env=vulkan1.0 -fshader-stage=vertex glyph.vert -o glyph.vert.spv
glslc --target-env=vulkan1.0 -fshader-stage=fragment glyph.frag -o glyph.frag.spv
sha256sum --check glyph.sha256
```

Then run the ignored `blend_pipeline_is_accepted_by_headless_vulkan` test with
Mesa's software ICD and the normal real-GPU capture probes documented in the
project `TESTING.md`.

`replay_rgba_to_nv12.comp` is the original GPU-only color-conversion stage for
Instant Replay. The capture layer exports a Vulkan buffer DMA-BUF, so the
shader reads it as a packed `std430` storage buffer using the validated byte
offset and row stride; it must never be rebound as an image. One invocation
converts a 2x2 8-bit RGBA/BGRA or packed 10-bit RGB block into four BT.709 limited-range luma samples and
one averaged interleaved chroma sample. The shader writes separate `r8` and
`rg8` device-local storage images. A following GPU image copy transfers those
images into the NV12 plane aspects; the encoder never reads the intermediate
images directly. H.264 declares matching BT.709 limited-range VUI metadata.
Packed 10-bit UNORM inputs using the standard sRGB nonlinear swapchain color
space are normalized directly. HDR color spaces remain outside the eligible
capture contract until a measured tone-mapping policy exists.

The production capture library includes the adjacent checked-in
`replay_rgba_to_nv12.comp.spv`; Redunar never compiles shaders in the installed
application. The binary was generated from the original source with Shaderc
2026.1, targeting Vulkan 1.0 so it remains valid on every Vulkan Video 1.3
device. Its source and binary hashes are pinned in
`replay_rgba_to_nv12.sha256`.

`replay_kms_rgba_to_nv12.comp` is the production-candidate variant for a
single-plane packed RGB KMS framebuffer imported as a sampled Vulkan image with
its explicit DRM modifier. It shares the same output color contract and never
maps source pixels into host memory. Its checked-in SPIR-V and
`replay_kms_rgba_to_nv12.sha256` are verified by the same release script.

Every development and release check can verify the checked-in source, binary,
SPIR-V size, and magic without a shader compiler:

```bash
tools/check-replay-shader.sh --check
```

When `spirv-val` is installed, that command additionally performs Khronos
SPIR-V validation. To prove that the documented Shaderc toolchain reproduces
the binary byte-for-byte, use:

```bash
tools/check-replay-shader.sh --recompile
```

After intentionally reviewing a shader change, regenerate the binary and its
hash manifest with `--update`. This development-only action requires `glslc`;
it does not add Shaderc, GLSL, or runtime compilation to Redunar. Review both
the GLSL and resulting hash change before accepting it.
