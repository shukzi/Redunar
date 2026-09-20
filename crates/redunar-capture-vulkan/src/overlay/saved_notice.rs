//! A fixed-size save confirmation, independent of metric layout and visibility.
use super::{OverlayCorner, OverlayPlacement, OverlayPlan, push_ui_text, push_ui_text_scaled};

pub(super) fn plan() -> OverlayPlan {
    let mut plan = OverlayPlan::hidden();
    plan.width = 340;
    plan.height = 88;
    plan.corner = OverlayCorner::BottomLeft;
    plan.opacity_percent = 100;
    plan.scale_percent = 100;
    plan.placement = OverlayPlacement::SavedNotice;
    plan.animation_progress = u16::MAX;

    // Ten-pixel corners built once from bounded horizontal spans; no texture,
    // allocation, or new graphics pipeline on the game's presentation path.
    for y in 0_i32..10 {
        let dy = 9 - y;
        let mut inset = 0;
        while (10 - inset) * (10 - inset) + dy * dy > 100 {
            inset += 1;
        }
        let width = u32::try_from(340 - inset * 2).unwrap_or(0);
        for row in [y, 87 - y] {
            plan.logo_base.push(inset, row, width, 1);
            if y == 0 {
                plan.dividers.push(inset, row, width, 1);
            } else {
                plan.dividers.push(inset, row, 1, 1);
                plan.dividers.push(339 - inset, row, 1, 1);
            }
        }
    }
    plan.logo_base.push(0, 10, 340, 68);
    plan.dividers.push(0, 10, 1, 68);
    plan.dividers.push(339, 10, 1, 68);
    // Red down-left save arrow, and a quiet check on the opposite edge.
    plan.accent.push(24, 37, 2, 15);
    plan.accent.push(24, 50, 14, 2);
    for i in 0..13 {
        plan.accent.push(25 + i, 49 - i, 2, 2);
    }
    for i in 0..4 {
        plan.cursor.push(307 + i, 43 + i, 1, 2);
    }
    for i in 0..7 {
        plan.cursor.push(311 + i, 46 - i, 1, 2);
    }
    push_ui_text_scaled(&mut plan.text_glyphs, b"Moment saved.", 58, 17, 2);
    push_ui_text(
        &mut plan.muted_glyphs,
        b"Saved to your local library",
        58,
        54,
    );
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::VkExtent2d;
    use crate::overlay::{effective_scale_percent, overlay_origin};

    #[test]
    fn live_visibility_preserves_the_save_notice_and_metric_configuration() {
        use crate::overlay::{OverlayConfig, OverlayPreset, RendererState};
        let mut state = RendererState {
            metrics_visible: true,
            ..Default::default()
        };
        let mut telemetry = redunar_capture::OverlayHardwareTelemetry {
            revision: 2,
            corner: 3,
            preset: 1,
            layout: 0,
            palette: 0,
            metrics: 3,
            opacity_percent: 70,
            scale_percent: 125,
            replay_saved_revision: 1,
            metrics_visible: Some(false),
            branding_visible: true,
            cpu_utilization_tenths: Some(750),
            cpu_temperature_tenths_celsius: None,
            gpu_utilization_tenths: None,
            gpu_temperature_tenths_celsius: None,
        };
        state.update_presentation(Some(telemetry), None, 1_000_000);
        assert!(state.plan.is_empty());
        assert!(!state.saved_notice.is_empty());
        assert_eq!(state.history.snapshot.cpu_percent, Some(75));
        telemetry.metrics_visible = Some(true);
        telemetry.revision = 4;
        state.update_presentation(Some(telemetry), None, 2_000_000);
        assert!(!state.plan.is_empty());
        assert!(!state.saved_notice.is_empty());
        assert_eq!(state.plan.corner, OverlayCorner::BottomRight);
        assert_eq!(state.plan.scale_percent, 125);
        assert_eq!(state.plan.opacity_percent, 70);
        assert_eq!(
            state.config,
            OverlayConfig {
                corner: OverlayCorner::BottomRight,
                preset: OverlayPreset::Detailed,
                layout: crate::overlay::OverlayLayout::Grid,
                palette: crate::overlay::OverlayPalette::Redunar,
                branding_visible: true,
                metrics: 3,
                opacity_percent: 70,
                scale_percent: 125,
            }
        );
        telemetry.metrics_visible = Some(false);
        state.update_presentation(Some(telemetry), None, 3_000_000);
        state.update_presentation(None, None, 4_000_000_000);
        assert!(state.plan.is_empty());
        assert!(state.saved_notice.is_empty());
        assert!(
            state.history.length > 0,
            "hidden overlay still measures frames"
        );
    }

    #[test]
    fn notice_lifetime_is_independent_of_metrics_and_expires_after_last_save() {
        let mut state = crate::overlay::RendererState {
            metrics_visible: false,
            plan: OverlayPlan::hidden(),
            ..Default::default()
        };
        assert!(!state.update_saved_notice(Some(0), 0));
        assert!(state.saved_notice.is_empty());
        assert!(state.update_saved_notice(Some(1), 10));
        assert!(state.plan.is_empty());
        assert!(!state.saved_notice.is_empty());
        assert!(!state.update_saved_notice(Some(1), 1_000_000_010));
        assert_eq!(state.replay_saved_until_ns, 3_000_000_010);
        assert!(!state.update_saved_notice(Some(2), 2_000_000_010));
        assert_eq!(state.replay_saved_until_ns, 5_000_000_010);
        assert!(!state.update_saved_notice(None, 5_000_000_009));
        assert!(state.update_saved_notice(None, 5_000_000_010));
        assert!(state.saved_notice.is_empty());
        assert!(!state.update_saved_notice(Some(2), 6_000_000_010));
        assert!(state.update_saved_notice(Some(3), 7_000_000_010));
    }

    #[test]
    fn pill_is_bounded_and_anchored_bottom_left() {
        let plan = plan();
        assert!(!plan.is_empty());
        assert_eq!(plan.corner, OverlayCorner::BottomLeft);
        assert_eq!(plan.logo_base.length, 21);
        assert!(plan.dividers.length <= 48);
        for extent in [
            VkExtent2d {
                width: 1920,
                height: 1080,
            },
            VkExtent2d {
                width: 640,
                height: 480,
            },
        ] {
            let scale = effective_scale_percent(extent, &plan);
            let (x, y) = overlay_origin(extent, &plan, scale);
            assert_eq!(x, 12);
            assert_eq!(
                y + i32::try_from(plan.height).unwrap(),
                i32::try_from(extent.height).unwrap() - 12
            );
        }
        for batch in [&plan.text_glyphs, &plan.muted_glyphs] {
            for glyph in batch.as_slice() {
                assert!(glyph.x >= 0 && glyph.y >= 0);
                assert!(glyph.x + 9 * i32::from(glyph.scale) <= 340);
                assert!(glyph.y + 12 * i32::from(glyph.scale) <= 88);
            }
        }
    }
}

#[cfg(test)]
#[test]
fn render_saved_pill_reference() {
    use redunar_core::overlay_font;
    use std::fmt::Write as _;
    let Ok(path) = std::env::var("REDUNAR_SAVED_PILL_REFERENCE") else {
        return;
    };
    let plan = plan();
    let mut svg = String::from(
        "<svg xmlns='http://www.w3.org/2000/svg' width='680' height='176' viewBox='0 0 340 88'>",
    );
    for (batch, color) in [
        (&plan.logo_base, "#131316"),
        (&plan.dividers, "#262930"),
        (&plan.accent, "#eb2933"),
        (&plan.cursor, "#a1c2b3"),
    ] {
        for rect in batch.as_slice() {
            let r = rect.rect;
            write!(
                svg,
                "<rect x='{}' y='{}' width='{}' height='{}' fill='{color}'/>",
                r.offset.x, r.offset.y, r.extent.width, r.extent.height
            )
            .unwrap();
        }
    }
    for (batch, color) in [
        (&plan.text_glyphs, "#ebf0f7"),
        (&plan.muted_glyphs, "#858a94"),
    ] {
        for glyph in batch.as_slice() {
            let raster = overlay_font::ui_coverage_raster(glyph.byte);
            for y in 0..24_u32 {
                for x in 0..18_u32 {
                    let coverage = overlay_font::coverage_pixel(raster, x, y);
                    if coverage == 0 {
                        continue;
                    }
                    let step = f64::from(glyph.scale) / 2.0;
                    write!(svg,"<rect x='{}' y='{}' width='{step}' height='{step}' fill='{color}' opacity='{}'/>",f64::from(glyph.x)+f64::from(x)*step,f64::from(glyph.y)+f64::from(y)*step,f64::from(coverage)/3.0).unwrap();
                }
            }
        }
    }
    svg.push_str("</svg>");
    std::fs::write(path, svg).unwrap();
}
