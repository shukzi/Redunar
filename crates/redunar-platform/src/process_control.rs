use std::io;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AffinityMask {
    pub cpus: Vec<usize>,
}

/// Read a process CPU affinity using the bounded `taskset` interface.
///
/// # Errors
///
/// Returns an error when the process cannot be inspected or output is malformed.
pub fn read_affinity(pid: libc::pid_t) -> io::Result<AffinityMask> {
    let output = std::process::Command::new("taskset")
        .args(["-pc", &pid.to_string()])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("taskset could not read process affinity"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let list = text
        .rsplit_once(':')
        .map(|(_, value)| value.trim())
        .ok_or_else(|| io::Error::other("taskset returned an invalid affinity"))?;
    Ok(AffinityMask {
        cpus: parse_cpu_list(list)?,
    })
}

/// Apply a validated CPU affinity to a process owned by the current user.
///
/// # Errors
///
/// Returns an error when the mask is empty, invalid, or the process rejects the change.
pub fn apply_affinity(pid: libc::pid_t, mask: &AffinityMask) -> io::Result<()> {
    if mask.cpus.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "affinity cannot be empty",
        ));
    }
    let list = mask
        .cpus
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let status = std::process::Command::new("taskset")
        .args(["-pc", &list, &pid.to_string()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("taskset could not apply process affinity"))
    }
}

/// Read a process nice value.
///
/// # Errors
///
/// Returns an error when procps cannot inspect the process.
pub fn read_priority(pid: libc::pid_t) -> io::Result<i32> {
    let output = std::process::Command::new("ps")
        .args(["-o", "ni=", "-p", &pid.to_string()])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("ps could not read process priority"));
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| io::Error::other("ps returned an invalid priority"))
}

/// Apply a bounded nice adjustment to a process.
///
/// # Errors
///
/// Returns an error when the adjustment is out of range or permission is denied.
pub fn apply_priority(pid: libc::pid_t, value: i32) -> io::Result<()> {
    if !(-5..=5).contains(&value) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "priority adjustment is outside the supported range",
        ));
    }
    let status = std::process::Command::new("renice")
        .args(["-n", &value.to_string(), "-p", &pid.to_string()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("renice could not apply process priority"))
    }
}

fn parse_cpu_list(value: &str) -> io::Result<Vec<usize>> {
    let mut cpus = Vec::new();
    for part in value.split(',') {
        if let Some((start, end)) = part.split_once('-') {
            let start: usize = start
                .trim()
                .parse()
                .map_err(|_| io::Error::other("invalid CPU range"))?;
            let end: usize = end
                .trim()
                .parse()
                .map_err(|_| io::Error::other("invalid CPU range"))?;
            if end < start || end - start > 4096 {
                return Err(io::Error::other("CPU range is unbounded"));
            }
            cpus.extend(start..=end);
        } else {
            cpus.push(
                part.trim()
                    .parse()
                    .map_err(|_| io::Error::other("invalid CPU index"))?,
            );
        }
    }
    Ok(cpus)
}
