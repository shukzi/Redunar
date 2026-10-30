//! Redunar's small, deterministic antialiased overlay typeface.
//!
//! The metric consumes a bounded Noto Sans Mono subset while the Replay menu
//! uses a proportional Noto Sans subset matching the desktop hierarchy.
//! Layout remains on a 9x12 logical cell while the Vulkan renderer samples a
//! genuine 18x24 raster. At 200% overlay scale that is one source coverage
//! sample per output pixel rather than enlarged bitmap squares.

#![expect(
    clippy::unreadable_literal,
    reason = "unseparated nine-bit rows preserve the visual left-to-right glyph grid"
)]

pub const GLYPH_COLUMNS: u32 = 9;
pub const GLYPH_ROWS: u32 = 12;
pub const PIXEL_X_PITCH: u32 = 1;
pub const PIXEL_Y_PITCH: u32 = 1;
pub const PIXEL_WIDTH: u32 = 1;
pub const PIXEL_HEIGHT: u32 = 1;
pub const GLYPH_ADVANCE: i32 = 9;
// Fixed transport canvas shared with the descriptor-free Vulkan shader. The
// final column is deliberate padding and must not be derived from stroke size.
pub const GLYPH_WIDTH: u32 = 9;
pub const GLYPH_HEIGHT: u32 = 12;
pub const RASTER_WORDS: usize = 4;
pub const COVERAGE_WIDTH: u32 = 18;
pub const COVERAGE_HEIGHT: u32 = 24;
pub const COVERAGE_BITS: u32 = 2;
/// Two-bit coverage keeps the genuine 2x source raster in a 108-byte payload,
/// below Vulkan's portable 128-byte push-constant minimum.
pub const COVERAGE_WORDS: usize = 27;

/// Return twelve top-to-bottom rows. Bit 8 is the left edge and bit 0 is the
/// right edge. These rows are a thresholded subset of Noto Sans Mono Regular;
/// see `packaging/THIRD-PARTY-NOTICES.md` for provenance and license details.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "the complete bounded licensed glyph subset stays auditable in one match table"
)]
pub const fn glyph(byte: u8) -> [u16; GLYPH_ROWS as usize] {
    match byte {
        // The runtime protocol uses these two otherwise-unused ASCII slots for
        // the middle dot and degree mark in compact hardware readings.
        b'~' => [0, 0, 0, 0, 0, 0, 0b000110000, 0b000110000, 0, 0, 0, 0],
        b'^' => [
            0,
            0b001110000,
            0b010001000,
            0b010001000,
            0b001110000,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        b'-' => [0, 0, 0, 0, 0, 0, 0b001111000, 0, 0, 0, 0, 0],
        b'.' => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0b000110000, 0b000110000, 0],
        b'%' => [
            0,
            0b111100100,
            0b100101100,
            0b100101000,
            0b111111000,
            0b000010000,
            0b000111110,
            0b000111010,
            0b001111011,
            0b001011010,
            0b011001110,
            0,
        ],
        b'0' => [
            0,
            0b001111000,
            0b011001100,
            0b011001100,
            0b010011110,
            0b010010110,
            0b010110110,
            0b011100110,
            0b011000100,
            0b011001100,
            0b001111000,
            0,
        ],
        b'1' => [
            0,
            0b000110000,
            0b011110000,
            0b000010000,
            0b000010000,
            0b000010000,
            0b000010000,
            0b000010000,
            0b000010000,
            0b000010000,
            0b001111100,
            0,
        ],
        b'2' => [
            0,
            0b001111000,
            0b011001100,
            0b000000100,
            0b000000100,
            0b000001100,
            0b000011000,
            0b000110000,
            0b001100000,
            0b011000000,
            0b011111110,
            0,
        ],
        b'3' => [
            0,
            0b001111000,
            0b011001100,
            0b000000100,
            0b000001100,
            0b001111000,
            0b000001100,
            0b000000100,
            0b000000100,
            0b010001100,
            0b011111000,
            0,
        ],
        b'4' => [
            0,
            0b000011000,
            0b000011000,
            0b000111000,
            0b000101000,
            0b001101000,
            0b011001000,
            0b010001000,
            0b011111110,
            0b000001100,
            0b000001000,
            0,
        ],
        b'5' => [
            0,
            0b011111100,
            0b011000000,
            0b011000000,
            0b011111000,
            0b000011100,
            0b000000100,
            0b000000100,
            0b000000100,
            0b010001100,
            0b011111000,
            0,
        ],
        b'6' => [
            0,
            0b000111100,
            0b001100000,
            0b011000000,
            0b011111000,
            0b011001100,
            0b010000110,
            0b010000110,
            0b011000110,
            0b011001100,
            0b001111000,
            0,
        ],
        b'7' => [
            0,
            0b011111110,
            0b000000110,
            0b000001100,
            0b000001100,
            0b000001000,
            0b000011000,
            0b000010000,
            0b000110000,
            0b000110000,
            0b001100000,
            0,
        ],
        b'8' => [
            0,
            0b001111000,
            0b011001100,
            0b011000100,
            0b011001100,
            0b001111000,
            0b001111100,
            0b011000100,
            0b010000110,
            0b011001100,
            0b001111000,
            0,
        ],
        b'9' => [
            0,
            0b001111000,
            0b011001100,
            0b010000100,
            0b010000110,
            0b011001110,
            0b001111110,
            0b000000110,
            0b000001100,
            0b000011100,
            0b001111000,
            0,
        ],
        b'A' => [
            0,
            0b000110000,
            0b000111000,
            0b000111000,
            0b001101000,
            0b001101100,
            0b001001100,
            0b011111100,
            0b011000100,
            0b010000110,
            0b110000110,
            0,
        ],
        b'C' => [
            0,
            0b000111110,
            0b001100000,
            0b011000000,
            0b011000000,
            0b010000000,
            0b010000000,
            0b011000000,
            0b011000000,
            0b001100000,
            0b000111100,
            0,
        ],
        b'D' => [
            0,
            0b011111000,
            0b010011100,
            0b010000100,
            0b010000110,
            0b010000110,
            0b010000110,
            0b010000110,
            0b010000100,
            0b010011100,
            0b011111000,
            0,
        ],
        b'E' => [
            0,
            0b011111100,
            0b011000000,
            0b011000000,
            0b011000000,
            0b011111100,
            0b011000000,
            0b011000000,
            0b011000000,
            0b011000000,
            0b011111100,
            0,
        ],
        b'F' => [
            0,
            0b011111100,
            0b011000000,
            0b011000000,
            0b011000000,
            0b011111100,
            0b011000000,
            0b011000000,
            0b011000000,
            0b011000000,
            0b011000000,
            0,
        ],
        b'G' => [
            0,
            0b001111100,
            0b011100100,
            0b011000000,
            0b010000000,
            0b110011110,
            0b110000110,
            0b010000110,
            0b011000110,
            0b011100110,
            0b001111100,
            0,
        ],
        b'M' => [
            0,
            0b011000110,
            0b011001110,
            0b011001110,
            0b011101110,
            0b010111110,
            0b010111110,
            0b010110110,
            0b010010110,
            0b010000110,
            0b010000110,
            0,
        ],
        b'N' => [
            0,
            0b011000110,
            0b011000110,
            0b011100110,
            0b010100110,
            0b010110110,
            0b010010110,
            0b010011110,
            0b010001110,
            0b010001110,
            0b010001110,
            0,
        ],
        b'P' => [
            0,
            0b011111100,
            0b011001100,
            0b011000110,
            0b011000110,
            0b011001100,
            0b011111100,
            0b011000000,
            0b011000000,
            0b011000000,
            0b011000000,
            0,
        ],
        b'R' => [
            0,
            0b011111000,
            0b011001100,
            0b011000100,
            0b011000100,
            0b011001100,
            0b011111000,
            0b011011000,
            0b011001100,
            0b011001100,
            0b011000110,
            0,
        ],
        b'S' => [
            0,
            0b001111100,
            0b011100100,
            0b011000000,
            0b011000000,
            0b001110000,
            0b000011100,
            0b000000100,
            0b000000110,
            0b010001100,
            0b011111000,
            0,
        ],
        b'T' => [
            0,
            0b111111110,
            0b000110000,
            0b000110000,
            0b000110000,
            0b000110000,
            0b000110000,
            0b000110000,
            0b000110000,
            0b000110000,
            0b000110000,
            0,
        ],
        b'U' => [
            0,
            0b010000110,
            0b010000110,
            0b010000110,
            0b010000110,
            0b010000110,
            0b010000110,
            0b010000110,
            0b011000100,
            0b011001100,
            0b001111000,
            0,
        ],
        b'V' => [
            0,
            0b110000110,
            0b010000110,
            0b011000100,
            0b011000100,
            0b001001100,
            0b001001000,
            0b001101000,
            0b000111000,
            0b000111000,
            0b000110000,
            0,
        ],
        _ => [0; GLYPH_ROWS as usize],
    }
}

/// Expand one logical glyph into the exact 9x12 binary raster consumed by the
/// Vulkan shader. Four scalar words avoid
/// driver-dependent array layout while keeping every glyph bounded to 16 bytes.
#[must_use]
pub const fn raster(byte: u8) -> [u32; RASTER_WORDS] {
    let rows = glyph(byte);
    let mut words = [0_u32; RASTER_WORDS];
    let mut row = 0_u32;
    while row < GLYPH_ROWS {
        let mut column = 0_u32;
        while column < GLYPH_COLUMNS {
            if rows[row as usize] & (1 << (GLYPH_COLUMNS - 1 - column)) != 0 {
                let mut pixel_y = row * PIXEL_Y_PITCH;
                while pixel_y < row * PIXEL_Y_PITCH + PIXEL_HEIGHT {
                    let mut pixel_x = column * PIXEL_X_PITCH;
                    while pixel_x < column * PIXEL_X_PITCH + PIXEL_WIDTH {
                        let index = pixel_y * GLYPH_WIDTH + pixel_x;
                        words[(index / 32) as usize] |= 1 << (index % 32);
                        pixel_x += 1;
                    }
                    pixel_y += 1;
                }
            }
            column += 1;
        }
        row += 1;
    }
    words
}

include!("overlay_font_coverage.rs");
include!("overlay_ui_font_coverage.rs");

/// Return the checked-in 18x24 Noto Sans Mono coverage raster consumed by the
/// injected Vulkan renderer. Values use two bits per source pixel.
#[must_use]
pub const fn coverage_raster(byte: u8) -> [u32; COVERAGE_WORDS] {
    high_resolution_coverage(byte)
}

/// Return the proportional Noto Sans coverage used by the large Replay menu.
#[must_use]
pub const fn ui_coverage_raster(byte: u8) -> [u32; COVERAGE_WORDS] {
    ui_high_resolution_coverage(byte)
}

/// Return the proportional UI glyph advance in logical overlay pixels.
#[must_use]
pub const fn ui_advance_width(byte: u8) -> i32 {
    ui_advance(byte)
}

/// Return Redunar's antialiased upper-left pointer used by the Replay menu.
#[must_use]
pub const fn cursor_coverage_raster() -> [u32; COVERAGE_WORDS] {
    [
        0x00000000, 0x00000140, 0x00003800, 0x001f4000, 0x07f40000, 0xff400000, 0xf4000001,
        0x400000bf, 0x00002fff, 0x000bfff4, 0x02ffff40, 0xfffff400, 0xffff4000, 0xfff4003f,
        0xff401fff, 0xf402aabf, 0x400003ff, 0x00007f3f, 0x000fd0f4, 0x01fc0380, 0x3f400400,
        0xf0000000, 0x0000000b, 0x000000fd, 0x000007c0, 0x00000000, 0x00000000,
    ]
}

/// Return the matching one-pixel cursor outline. Keeping the outline in its
/// own raster avoids the doubled, offset silhouette produced by a drop-shadow
/// copy and remains legible on both bright and dark game frames.
#[must_use]
pub const fn cursor_outline_coverage_raster() -> [u32; COVERAGE_WORDS] {
    [
        0x0000003e, 0x00001fe0, 0x0007fe00, 0x01ffe000, 0x7ffe0000, 0xffe00000, 0xfe00002f,
        0xe0000bff, 0x0002ffff, 0x00bffffe, 0x3fffffe0, 0xfffffe00, 0xffffe00f, 0xfffe03ff,
        0xffe0ffff, 0xfe1fffff, 0xe0aabfff, 0x0003ffff, 0x007ffffe, 0x0fff7fe0, 0xfff0fe00,
        0xfd02d002, 0xc000003f, 0x000007ff, 0x00003ff4, 0x0001ff00, 0x00014000,
    ]
}

#[must_use]
pub const fn coverage_pixel(words: [u32; COVERAGE_WORDS], x: u32, y: u32) -> u8 {
    if x >= COVERAGE_WIDTH || y >= COVERAGE_HEIGHT {
        return 0;
    }
    let bit_index = (y * COVERAGE_WIDTH + x) * COVERAGE_BITS;
    ((words[(bit_index / 32) as usize] >> (bit_index % 32)) & 0b11) as u8
}

#[must_use]
pub const fn raster_pixel(words: [u32; RASTER_WORDS], x: u32, y: u32) -> bool {
    if x >= GLYPH_WIDTH || y >= GLYPH_HEIGHT {
        return false;
    }
    let index = y * GLYPH_WIDTH + x;
    words[(index / 32) as usize] & (1 << (index % 32)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_abi_and_thin_strokes_are_preserved() {
        assert_eq!(GLYPH_WIDTH, 9);
        assert_eq!(GLYPH_HEIGHT, 12);
        assert_eq!(GLYPH_ADVANCE, 9);
        assert_eq!(glyph(b'8')[1], 0b001111000);
        assert_eq!(glyph(b'8')[10], 0b001111000);
        let eight = raster(b'8');
        assert!(raster_pixel(eight, 2, 1));
        assert!(raster_pixel(eight, 5, 1));
        assert!(!raster_pixel(eight, 0, 0));
        assert!(!raster_pixel(eight, 0, 1));
        assert!(!raster_pixel(eight, 8, 1));
        assert!(!raster_pixel(eight, GLYPH_WIDTH, 0));
    }

    #[test]
    fn every_supported_glyph_fits_the_nine_pixel_canvas() {
        for byte in b"^~-.%0123456789ACDEFGMNPRSTUV" {
            assert!(glyph(*byte).iter().all(|row| row & !0b1_1111_1111 == 0));
        }
    }

    #[test]
    fn expanded_raster_preserves_rows_across_every_word_boundary() {
        for byte in b"^~-.%0123456789ACDEFGMNPRSTUV" {
            let rows = glyph(*byte);
            let pixels = raster(*byte);
            for y in 0..GLYPH_HEIGHT {
                for x in 0..GLYPH_WIDTH {
                    let expected = rows[y as usize] & (1 << (GLYPH_COLUMNS - 1 - x)) != 0;
                    assert_eq!(
                        raster_pixel(pixels, x, y),
                        expected,
                        "glyph {byte:#04x} differs at ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn runtime_coverage_is_a_genuine_two_x_antialiased_raster() {
        assert_eq!(COVERAGE_WIDTH, GLYPH_WIDTH * 2);
        assert_eq!(COVERAGE_HEIGHT, GLYPH_HEIGHT * 2);
        assert_eq!(COVERAGE_WORDS * 4, 108);
        let eight = coverage_raster(b'8');
        let samples = (0..COVERAGE_HEIGHT)
            .flat_map(|y| (0..COVERAGE_WIDTH).map(move |x| coverage_pixel(eight, x, y)))
            .collect::<Vec<_>>();
        assert!(samples.contains(&1));
        assert!(samples.contains(&2));
        assert!(samples.contains(&3));
        for byte in b"*+/ABCDEFGHIJKLMNOPQRSTUVWXYZ" {
            assert!(
                coverage_raster(*byte).iter().any(|word| *word != 0),
                "runtime UI glyph {byte:#04x} is missing"
            );
        }
        for byte in b"-.%*+/0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz" {
            assert!(
                ui_coverage_raster(*byte).iter().any(|word| *word != 0),
                "proportional UI glyph {byte:#04x} is missing"
            );
            assert!(ui_advance_width(*byte) > 0);
        }
        assert_eq!(ui_advance_width(b' '), 4);
    }
}
