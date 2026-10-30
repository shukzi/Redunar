use evdev::Device;
use std::io;

// Each fetch consumes a finite evdev batch. A busy/broken device must not keep
// the shortcut worker in queue cleanup indefinitely; retain view-only fallback.
const MAX_DRAIN_BATCHES: usize = 32;

pub(crate) trait PointerDevice {
    fn grab(&mut self) -> io::Result<()>;
    fn ungrab(&mut self) -> io::Result<()>;
    fn discard_events(&mut self) -> io::Result<()>;
}

impl PointerDevice for Device {
    fn grab(&mut self) -> io::Result<()> {
        Device::grab(self)
    }

    fn ungrab(&mut self) -> io::Result<()> {
        Device::ungrab(self)
    }

    fn discard_events(&mut self) -> io::Result<()> {
        // Consume the full iterator so evdev's synchronized state advances too.
        self.fetch_events()?.for_each(|_| {});
        Ok(())
    }
}

pub(crate) fn grab_all(devices: &mut [impl PointerDevice]) -> io::Result<()> {
    let result = prepare(devices);
    if result.is_err() {
        release_all(devices);
    }
    result
}

fn prepare(devices: &mut [impl PointerDevice]) -> io::Result<()> {
    for device in devices.iter_mut() {
        device.grab()?;
    }
    // These handles collect gameplay input while the menu is closed. Drain
    // after taking ownership so old clicks/motion cannot dismiss or activate
    // the newly opened menu. New input is forwarded only after this boundary.
    for device in devices {
        let mut empty = false;
        for _ in 0..MAX_DRAIN_BATCHES {
            match device.discard_events() {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    empty = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if !empty {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "mouse input queue did not become empty",
            ));
        }
    }
    Ok(())
}

pub(crate) fn release_all(devices: &mut [impl PointerDevice]) {
    for device in devices {
        let _ = device.ungrab();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PointerEvent;
    use std::collections::VecDeque;

    #[derive(Default)]
    enum Behavior {
        #[default]
        Normal,
        FailGrab,
        FailRead,
        Busy,
    }

    #[derive(Default)]
    struct Mouse {
        grabbed: bool,
        released: bool,
        behavior: Behavior,
        batches: VecDeque<Vec<PointerEvent>>,
        reads: usize,
    }

    impl PointerDevice for Mouse {
        fn grab(&mut self) -> io::Result<()> {
            if matches!(self.behavior, Behavior::FailGrab) {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            self.grabbed = true;
            Ok(())
        }

        fn ungrab(&mut self) -> io::Result<()> {
            self.grabbed = false;
            self.released = true;
            Ok(())
        }

        fn discard_events(&mut self) -> io::Result<()> {
            assert!(self.grabbed, "cleanup follows pointer ownership");
            self.reads += 1;
            if matches!(self.behavior, Behavior::FailRead) {
                return Err(io::ErrorKind::NotConnected.into());
            }
            if matches!(self.behavior, Behavior::Busy) || self.batches.pop_front().is_some() {
                Ok(())
            } else {
                Err(io::ErrorKind::WouldBlock.into())
            }
        }
    }

    #[test]
    fn gameplay_clicks_and_motion_are_discarded_before_each_menu_session() {
        let mut mice = [Mouse::default(), Mouse::default()];
        for _ in 0..2 {
            for mouse in &mut mice {
                mouse.batches.extend([
                    vec![PointerEvent::MotionX(-4_096)],
                    vec![PointerEvent::LeftButton(true)],
                    vec![PointerEvent::LeftButton(false)],
                ]);
            }
            grab_all(&mut mice).unwrap();
            for mouse in &mut mice {
                assert!(mouse.grabbed);
                assert!(mouse.batches.is_empty());
                // A click arriving after cleanup remains available to the
                // normal menu polling path, instead of being discarded later.
                let fresh = vec![PointerEvent::LeftButton(true)];
                mouse.batches.push_back(fresh.clone());
                assert_eq!(mouse.batches.pop_front(), Some(fresh));
            }
            release_all(&mut mice);
            assert!(mice.iter().all(|mouse| !mouse.grabbed));
        }
    }

    #[test]
    fn failed_grab_or_drain_releases_every_mouse() {
        for behavior in [Behavior::FailGrab, Behavior::FailRead] {
            let mut mice = [
                Mouse::default(),
                Mouse {
                    behavior,
                    ..Mouse::default()
                },
            ];
            assert!(grab_all(&mut mice).is_err());
            assert!(mice.iter().all(|mouse| mouse.released && !mouse.grabbed));
        }
    }

    #[test]
    fn continuous_input_has_a_bounded_cleanup_and_releases_ownership() {
        let mut mice = [Mouse {
            behavior: Behavior::Busy,
            ..Mouse::default()
        }];
        assert_eq!(
            grab_all(&mut mice).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(mice[0].reads, MAX_DRAIN_BATCHES);
        assert!(mice[0].released && !mice[0].grabbed);
    }
}
