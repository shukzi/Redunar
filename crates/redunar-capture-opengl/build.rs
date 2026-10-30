use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/replay_export/nvidia_memory.c");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let object = out.join("nvidia_memory.o");
    let library = out.join("libredunar_nvidia_memory.a");
    let compiler = env::var_os("CC").unwrap_or_else(|| "cc".into());
    let archive = env::var_os("AR").unwrap_or_else(|| "ar".into());
    assert!(
        Command::new(compiler)
            .args([
                "-std=c11",
                "-O2",
                "-fPIC",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-c",
                "src/replay_export/nvidia_memory.c",
                "-o"
            ])
            .arg(&object)
            .status()
            .expect("C compiler and Vulkan development headers")
            .success()
    );
    assert!(
        Command::new(archive)
            .arg("crs")
            .arg(&library)
            .arg(&object)
            .status()
            .expect("static archiver")
            .success()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=redunar_nvidia_memory");
    println!("cargo:rustc-link-lib=dl");
    println!("cargo:rustc-link-lib=pthread");
}
