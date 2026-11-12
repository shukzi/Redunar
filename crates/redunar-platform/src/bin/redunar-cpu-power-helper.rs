//! Fixed, read-only CPU package power endpoint. Invoked through packaged polkit policy.
use std::io::{Read, Write};
use std::time::{Duration, Instant};
#[path = "../linux_cpu_metrics.rs"]
#[allow(dead_code)]
mod cpu_metrics;

fn main() {
    if std::env::args_os().len() != 1 || serve().is_err() {
        std::process::exit(1);
    }
}

fn serve() -> std::io::Result<()> {
    // No caller-controlled paths, commands or environment values are consumed.
    let mut metrics = cpu_metrics::CpuMetrics::discover(std::path::Path::new("/sys"));
    metrics.power_only();
    serve_samples(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        || metrics.sample().1,
        Duration::from_secs(1),
    )
}

fn serve_samples(
    mut input: impl Read,
    mut output: impl Write,
    mut sample: impl FnMut() -> Option<f64>,
    interval: Duration,
) -> std::io::Result<()> {
    let mut last = None::<Instant>;
    loop {
        let mut request = [0];
        match input.read_exact(&mut request) {
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            result => result?,
        }
        if request != *b"S" {
            return Err(std::io::Error::other("invalid sampling request"));
        }
        if let Some(last) = last {
            std::thread::sleep(interval.saturating_sub(last.elapsed()));
        }
        last = Some(Instant::now());
        // Only coarse package watts leave this process. Never expose raw energy.
        match sample() {
            Some(watts) => writeln!(output, "RCPU1 {:.0}", watts * 1000.0)?,
            None => writeln!(output, "RCPU1 -")?,
        }
        output.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_is_bounded_rate_limited_and_exits_on_disconnect() {
        let mut output = Vec::new();
        let start = Instant::now();
        serve_samples(
            &b"SSS"[..],
            &mut output,
            || Some(65.5),
            Duration::from_millis(10),
        )
        .unwrap();
        assert!(start.elapsed() >= Duration::from_millis(20));
        assert_eq!(output, b"RCPU1 65500\nRCPU1 65500\nRCPU1 65500\n");
        output.clear();
        serve_samples(&b"S"[..], &mut output, || None, Duration::ZERO).unwrap();
        assert_eq!(output, b"RCPU1 -\n");
        assert!(
            serve_samples(
                &b"/sys"[..],
                Vec::new(),
                || panic!("invalid request sampled"),
                Duration::ZERO
            )
            .is_err()
        );
    }
}
