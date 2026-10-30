//! Bounded reads of the driver's recent power samples, with legacy fallback.
use crate::NvmlError;
use crate::library::Device;
use std::time::{SystemTime, UNIX_EPOCH};

const CAPACITY: usize = 256;
const MAX_AGE_US: u64 = 2_000_000;
const POWER_SAMPLES: u32 = 0;
const UNSIGNED_INT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union Value {
    unsigned_int: u32,
    alignment: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Sample {
    timestamp: u64,
    value: Value,
}

impl Sample {
    const EMPTY: Self = Self {
        timestamp: 0,
        value: Value { alignment: 0 },
    };
}

pub(crate) type GetSamples =
    unsafe extern "C" fn(Device, u32, u64, *mut u32, *mut u32, *mut Sample) -> i32;

pub(crate) fn read(get: Option<GetSamples>, device: Device) -> Option<(i32, Option<f64>)> {
    let get = get?;
    let now = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_micros(),
    )
    .ok()?;
    read_at(get, device, now)
}

fn read_at(get: GetSamples, device: Device, now: u64) -> Option<(i32, Option<f64>)> {
    let mut samples = [Sample::EMPTY; CAPACITY];
    let mut count = u32::try_from(CAPACITY).ok()?;
    let mut value_type = u32::MAX;
    // SAFETY: the session owns this PCI-matched handle and loaded symbol.
    // The fixed buffer and input count agree; no driver count grows allocation.
    let result = unsafe {
        get(
            device,
            POWER_SAMPLES,
            now.saturating_sub(MAX_AGE_US),
            &raw mut value_type,
            &raw mut count,
            samples.as_mut_ptr(),
        )
    };
    if result != 0 {
        // Never hide device loss by attempting another query on its stale
        // handle. Unsupported/no-data/oversized buffers use the legacy call.
        return NvmlError::from_code(result)
            .invalidates_session()
            .then_some((result, None));
    }
    let count = usize::try_from(count).ok()?;
    if count == 0 || count > CAPACITY || value_type != UNSIGNED_INT {
        return None;
    }
    let newest = samples[..count]
        .iter()
        .filter(|sample| sample.timestamp <= now && now - sample.timestamp <= MAX_AGE_US)
        .max_by_key(|sample| sample.timestamp)?;
    // SAFETY: NVML reports the unsigned-int discriminator for these samples.
    Some((
        0,
        Some(f64::from(unsafe { newest.value.unsigned_int }) / 1_000.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    const NOW: u64 = 3_000_000;
    struct Reply {
        result: i32,
        value_type: u32,
        count: u32,
        timestamp: u64,
    }
    thread_local! { static REPLY: RefCell<Reply> = const { RefCell::new(Reply { result: 0, value_type: UNSIGNED_INT, count: 2, timestamp: NOW - 100_000 }) }; }
    unsafe extern "C" fn get(
        _: Device,
        kind: u32,
        since: u64,
        value_type: *mut u32,
        count: *mut u32,
        samples: *mut Sample,
    ) -> i32 {
        assert_eq!(kind, POWER_SAMPLES);
        assert_eq!(since, NOW - MAX_AGE_US);
        assert_eq!(
            unsafe { *count },
            u32::try_from(CAPACITY).expect("bounded capacity")
        );
        REPLY.with(|r| {
            let r = r.borrow();
            // Return newest first to cover drivers whose samples are not sorted.
            unsafe {
                *count = r.count;
                *value_type = r.value_type;
                *samples = Sample {
                    timestamp: r.timestamp,
                    value: Value {
                        unsigned_int: 42_000,
                    },
                };
                *samples.add(1) = Sample {
                    timestamp: NOW - 200_000,
                    value: Value {
                        unsigned_int: 12_000,
                    },
                };
            }
            r.result
        })
    }
    #[test]
    fn newest_recent_sample_uses_milliwatts_and_public_abi() {
        assert_eq!(std::mem::size_of::<Sample>(), 16);
        assert_eq!(std::mem::align_of::<Sample>(), 8);
        assert_eq!(
            read_at(get, std::ptr::null_mut(), NOW),
            Some((0, Some(42.0)))
        );
    }
    #[test]
    fn unsupported_and_missing_buffers_fall_back_without_inventing_zero() {
        for result in [3, 6, 7] {
            REPLY.with(|r| r.borrow_mut().result = result);
            assert_eq!(read_at(get, std::ptr::null_mut(), NOW), None);
        }
        assert_eq!(read(None, std::ptr::null_mut()), None);
    }
    #[test]
    fn device_loss_is_returned_without_legacy_retry() {
        REPLY.with(|r| r.borrow_mut().result = 15);
        assert_eq!(read_at(get, std::ptr::null_mut(), NOW), Some((15, None)));
    }
    #[test]
    fn wrong_type_and_oversized_or_empty_counts_fall_back() {
        REPLY.with(|r| r.borrow_mut().value_type = 0);
        assert_eq!(read_at(get, std::ptr::null_mut(), NOW), None);
        REPLY.with(|r| {
            let mut r = r.borrow_mut();
            r.value_type = UNSIGNED_INT;
            r.count = 257;
        });
        assert_eq!(read_at(get, std::ptr::null_mut(), NOW), None);
        REPLY.with(|r| r.borrow_mut().count = 0);
        assert_eq!(read_at(get, std::ptr::null_mut(), NOW), None);
    }
    #[test]
    fn stale_or_future_samples_never_become_current_readings() {
        REPLY.with(|r| {
            let mut r = r.borrow_mut();
            r.count = 1;
            r.timestamp = NOW - MAX_AGE_US - 1;
        });
        assert_eq!(read_at(get, std::ptr::null_mut(), NOW), None);
        REPLY.with(|r| r.borrow_mut().timestamp = NOW + 1);
        assert_eq!(read_at(get, std::ptr::null_mut(), NOW), None);
    }
}
