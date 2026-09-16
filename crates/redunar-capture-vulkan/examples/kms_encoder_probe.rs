use redunar_capture_vulkan::replay_video::{
    VulkanVideoH264Device, VulkanVideoH264Parameters, VulkanVideoH264Request,
    VulkanVideoH264Session,
};

fn main() {
    let request = VulkanVideoH264Request {
        width: 2_560,
        height: 1_440,
        frames_per_second: 60,
        target_megabits_per_second: 24,
    };
    match VulkanVideoH264Device::open(request)
        .and_then(VulkanVideoH264Device::create_session)
        .and_then(VulkanVideoH264Session::create_parameters)
        .and_then(VulkanVideoH264Parameters::create_production_kms_encoder)
    {
        Ok(encoder) => {
            println!(
                "kms_encoder_ready={}x{}@{}fps",
                encoder.request().width,
                encoder.request().height,
                encoder.request().frames_per_second
            );
            drop(encoder);
        }
        Err(error) => {
            eprintln!("kms_encoder_unavailable={error}");
            std::process::exit(1);
        }
    }
}
