fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        // The Vulkan loader exports the same command names as its layers.
        // Addresses returned by negotiation/GIPA must refer to our hooks,
        // even when a game linked libvulkan globally before loading us.
        // Keep this cdylib-only; next-layer calls still use captured pointers.
        println!("cargo:rustc-cdylib-link-arg=-Wl,-Bsymbolic-functions");
    }
}
