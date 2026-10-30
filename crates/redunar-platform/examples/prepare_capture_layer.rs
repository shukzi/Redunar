use redunar_platform::prepare_vulkan_layer_directory;
use std::path::PathBuf;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let session_directory = arguments
        .next()
        .map(PathBuf::from)
        .expect("usage: prepare_capture_layer SESSION_DIRECTORY LAYER_LIBRARY");
    let layer_library = arguments
        .next()
        .map(PathBuf::from)
        .expect("usage: prepare_capture_layer SESSION_DIRECTORY LAYER_LIBRARY");
    assert!(arguments.next().is_none(), "unexpected extra arguments");

    let directory = prepare_vulkan_layer_directory(session_directory, layer_library)
        .expect("prepare private Vulkan layer manifest");
    println!("{}", directory.display());
}
