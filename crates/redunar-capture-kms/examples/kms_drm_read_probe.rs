use redunar_capture_kms::KmsDrmCapture;

fn main() {
    let card_index = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(1);
    let connector = std::env::args().nth(2).unwrap_or_else(|| "DP-2".to_owned());

    match KmsDrmCapture::open(card_index, &connector) {
        Ok(capture) => {
            let (width, height) = capture.dimensions();
            println!("kms_drm_output_active=card{card_index}-{connector} {width}x{height}");
        }
        Err(error) => {
            eprintln!("kms_drm_output_unavailable=card{card_index}-{connector} {error}");
            std::process::exit(1);
        }
    }
}
