use redunar_daemon::RedunarService;

fn main() {
    let service = RedunarService::default();
    match service.snapshot() {
        Ok(snapshot) => {
            println!("CPU: {}", snapshot.cpu.model);
            for gpu in snapshot.gpus {
                println!("GPU: {} ({})", gpu.model, gpu.card);
            }
        }
        Err(error) => {
            redunar_daemon::log_op!("Redunar could not inspect this machine: {error}");
            std::process::exit(1);
        }
    }
}
