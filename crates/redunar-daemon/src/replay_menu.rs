use crate::{ReplayOutputFormat, ReplayPhase, ReplayRuntimeStatus};
use redunar_capture::{ReplayMenuStatus, ReplayMenuTelemetry};
use redunar_core::{ReplayDuration, ReplayFrameRate, ReplayQuality};
use std::time::{Duration, Instant};

const CURSOR_MAX: i32 = 10_000;
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(2);
const DURATIONS: [ReplayDuration; 8] = [
    ReplayDuration::Seconds15,
    ReplayDuration::Seconds30,
    ReplayDuration::Seconds60,
    ReplayDuration::Seconds120,
    ReplayDuration::Seconds180,
    ReplayDuration::Seconds300,
    ReplayDuration::Seconds600,
    ReplayDuration::Seconds900,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    None,
    Duration(u8),
    Save,
}

pub(crate) struct ReplayMenuState {
    visible: bool,
    cursor_x: i32,
    cursor_y: i32,
    pressed: bool,
    pressed_target: Target,
    selected: u8,
    click_revision: u16,
    last_input: Option<Instant>,
}

impl Default for ReplayMenuState {
    fn default() -> Self {
        Self {
            visible: false,
            cursor_x: 5_000,
            cursor_y: 5_000,
            pressed: false,
            pressed_target: Target::None,
            selected: 1,
            click_revision: 0,
            last_input: None,
        }
    }
}

impl ReplayMenuState {
    pub(crate) fn toggle(&mut self, now: Instant) -> bool {
        self.visible = !self.visible;
        self.pressed = false;
        self.pressed_target = Target::None;
        self.last_input = self.visible.then_some(now);
        self.visible
    }

    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.pressed = false;
        self.pressed_target = Target::None;
        self.last_input = None;
    }

    pub(crate) fn move_cursor(&mut self, dx: i32, dy: i32, now: Instant) {
        if self.visible {
            self.cursor_x = (self.cursor_x + dx).clamp(0, CURSOR_MAX);
            self.cursor_y = (self.cursor_y + dy).clamp(0, CURSOR_MAX);
            self.last_input = Some(now);
        }
    }

    pub(crate) fn button(
        &mut self,
        pressed: bool,
        replay_ready: bool,
        now: Instant,
    ) -> Option<ReplayDuration> {
        if !self.visible {
            return None;
        }
        self.last_input = Some(now);
        let target = hit_test(self.cursor_x, self.cursor_y);
        if pressed {
            self.pressed = true;
            self.pressed_target = target;
            return None;
        }
        let activated = self.pressed && self.pressed_target == target;
        self.pressed = false;
        self.pressed_target = Target::None;
        if !activated {
            return None;
        }
        match target {
            Target::Duration(index) => self.selected = index,
            Target::Save if replay_ready => {
                self.click_revision = self.click_revision.wrapping_add(1).max(1);
                let duration = DURATIONS.get(usize::from(self.selected)).copied();
                self.close();
                return duration;
            }
            Target::None => self.close(),
            Target::Save => {}
        }
        None
    }

    pub(crate) const fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn heartbeat(&mut self, now: Instant) {
        if self.visible {
            self.last_input = Some(now);
        }
    }

    pub(crate) fn close_if_stale(&mut self, now: Instant) -> bool {
        if self.visible
            && self
                .last_input
                .is_some_and(|last| now.saturating_duration_since(last) > WATCHDOG_TIMEOUT)
        {
            self.close();
            return true;
        }
        false
    }

    pub(crate) fn telemetry(
        &self,
        revision: u64,
        runtime: ReplayRuntimeStatus,
        format: ReplayOutputFormat,
    ) -> ReplayMenuTelemetry {
        ReplayMenuTelemetry {
            revision,
            visible: self.visible,
            pointer_pressed: self.pressed,
            cursor_x: u16::try_from(self.cursor_x).unwrap_or(0),
            cursor_y: u16::try_from(self.cursor_y).unwrap_or(0),
            hover_target: target_code(hit_test(self.cursor_x, self.cursor_y)),
            pressed_target: target_code(self.pressed_target),
            selected_duration_index: self.selected,
            status: status(runtime.phase),
            available_seconds: u16::try_from(runtime.buffered_duration_ns / 1_000_000_000)
                .unwrap_or(u16::MAX)
                .min(900),
            click_revision: self.click_revision,
            frame_rate: frame_rate(runtime.settings.frame_rate),
            quality: quality(runtime.settings.quality),
            output_format: u8::from(format == ReplayOutputFormat::Mp4),
            save_enabled: runtime.phase == ReplayPhase::Buffering
                && runtime.buffered_duration_ns >= 1_000_000_000,
        }
    }
}

fn hit_test(x: i32, y: i32) -> Target {
    // These normalized bounds are the exact 690x440 Vulkan menu grid:
    // x 28..424, duration y 238..324, save y 338..386.
    if (406..6_145).contains(&x) && (5_409..7_364).contains(&y) {
        let column = ((x - 406) * 4 / (6_145 - 406)).clamp(0, 3);
        let row = ((y - 5_409) * 2 / (7_364 - 5_409)).clamp(0, 1);
        return Target::Duration(u8::try_from(row * 4 + column).unwrap_or(0));
    }
    if (406..6_145).contains(&x) && (7_682..8_773).contains(&y) {
        return Target::Save;
    }
    Target::None
}

const fn target_code(target: Target) -> u8 {
    match target {
        Target::None => 0,
        Target::Duration(index) => index + 1,
        Target::Save => 9,
    }
}

const fn status(phase: ReplayPhase) -> ReplayMenuStatus {
    match phase {
        ReplayPhase::Unavailable => ReplayMenuStatus::Unavailable,
        ReplayPhase::Inactive => ReplayMenuStatus::Inactive,
        ReplayPhase::Buffering => ReplayMenuStatus::Buffering,
        ReplayPhase::Saving => ReplayMenuStatus::Saving,
        ReplayPhase::Failed => ReplayMenuStatus::Failed,
    }
}

const fn frame_rate(value: ReplayFrameRate) -> u8 {
    match value {
        ReplayFrameRate::Fps30 => 30,
        ReplayFrameRate::Fps60 => 60,
        ReplayFrameRate::Fps120 => 120,
    }
}

const fn quality(value: ReplayQuality) -> u8 {
    match value {
        ReplayQuality::Efficient => 0,
        ReplayQuality::Balanced => 1,
        ReplayQuality::High => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_activates_only_the_original_bounded_target_once() {
        let now = Instant::now();
        let mut state = ReplayMenuState::default();
        state.toggle(now);
        state.move_cursor(-2_000, 3_000, now);
        assert_eq!(state.button(true, true, now), None);
        assert_eq!(
            state.button(false, true, now),
            Some(ReplayDuration::Seconds30)
        );
        assert_eq!(state.button(false, true, now), None);
    }

    #[test]
    fn clicking_outside_closes_the_menu() {
        let now = Instant::now();
        let mut state = ReplayMenuState::default();
        state.toggle(now);
        state.move_cursor(4_900, 4_900, now);
        assert_eq!(state.button(true, true, now), None);
        assert_eq!(state.button(false, true, now), None);
        assert!(!state.is_visible());
    }

    #[test]
    fn duration_hit_test_changes_selection_without_saving() {
        let now = Instant::now();
        let mut state = ReplayMenuState::default();
        state.toggle(now);
        state.move_cursor(-4_000, 1_000, now);
        state.button(true, true, now);
        assert_eq!(state.button(false, true, now), None);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn every_duration_cell_selects_the_expected_bounded_option() {
        let now = Instant::now();
        let left = 406;
        let right = 6_145;
        let top = 5_409;
        let bottom = 7_364;
        let column_width = (right - left) / 4;
        let row_height = (bottom - top) / 2;

        for index in 0..DURATIONS.len() {
            let column = i32::try_from(index % 4).unwrap_or(0);
            let row = i32::try_from(index / 4).unwrap_or(0);
            let mut state = ReplayMenuState::default();
            state.toggle(now);
            state.cursor_x = left + column * column_width + column_width / 2;
            state.cursor_y = top + row * row_height + row_height / 2;
            assert_eq!(state.button(true, true, now), None);
            assert_eq!(state.button(false, true, now), None);
            assert_eq!(state.selected, u8::try_from(index).unwrap_or(0));
            assert!(state.is_visible());
        }
    }

    #[test]
    fn save_target_requires_ready_runtime_and_closes_after_one_activation() {
        let now = Instant::now();
        let mut state = ReplayMenuState::default();
        state.toggle(now);
        state.cursor_x = 3_000;
        state.cursor_y = 8_200;

        assert_eq!(state.button(true, false, now), None);
        assert_eq!(state.button(false, false, now), None);
        assert!(state.is_visible());

        assert_eq!(state.button(true, true, now), None);
        assert_eq!(
            state.button(false, true, now),
            Some(ReplayDuration::Seconds30)
        );
        assert!(!state.is_visible());
        assert_eq!(state.button(false, true, now), None);
    }

    #[test]
    fn watchdog_closes_and_cancels_a_pressed_button() {
        let now = Instant::now();
        let mut state = ReplayMenuState::default();
        state.toggle(now);
        state.button(true, true, now);
        assert!(state.close_if_stale(now + WATCHDOG_TIMEOUT + Duration::from_millis(1)));
        assert!(!state.visible);
        assert!(!state.pressed);
    }
}
