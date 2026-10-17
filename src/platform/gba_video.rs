//! Curated video treatments on the core worker; no shader runtime or I/O.
//! Each writes straight into Slint's pixel buffer, which the UI shows as is.
use super::gba_core::{Frame, Presented};
use crate::domain::core_settings::VideoFilter;
use slint::{Rgba8Pixel, SharedPixelBuffer};

/// The whole-number screen scale for a `source` frame in a `window` (both in
/// physical pixels), and the largest xBRZ factor (2–6) dividing it. The GPU
/// repeats each xBRZ pixel the rest of the way, so nothing is resampled by a
/// fraction. A scale no factor divides (7, 11, 13…) steps down by one,
/// leaving thin bars. None when the window cannot fit twice the source.
pub(super) fn xbrz_plan(source: (u32, u32), window: (u32, u32)) -> Option<(u32, u32)> {
    if source.0 == 0 || source.1 == 0 {
        return None;
    }
    let fit = (window.0 / source.0).min(window.1 / source.1);
    (2..=fit)
        .rev()
        .find_map(|scale| (2..=6).rev().find(|f| scale % f == 0).map(|f| (f, scale)))
}

/// The frame to show for `raw` (left untouched: screenshots use it).
/// `scratch` holds the colour-corrected copy between frames; `window` is
/// the player's size in physical pixels (0×0 when unknown).
pub(super) fn present(
    raw: &Frame,
    color_correction: bool,
    filter: VideoFilter,
    window: (u32, u32),
    scratch: &mut Vec<u8>,
) -> Presented {
    let (width, height) = (raw.width, raw.height);
    let source: &[u8] = if color_correction {
        scratch.clear();
        scratch.extend_from_slice(&raw.rgba);
        correct_color(scratch);
        scratch
    } else {
        &raw.rgba
    };
    let smooth = matches!(
        filter,
        VideoFilter::Smooth | VideoFilter::Lcd | VideoFilter::Xbrz
    );
    let whole = source.len() == width as usize * height as usize * 4;
    // Bound output allocation even if a future core reports larger frames.
    if filter == VideoFilter::Xbrz && whole && width <= 512 && height <= 512 {
        let plan = xbrz_plan((width, height), window);
        // Without a whole-number scale, reconstruct at 6× and fit smoothly,
        // so a small intermediate is not magnified into soft edges.
        let factor = plan.map_or(6, |(factor, _)| factor);
        let scaled = xbrz::scale_rgba(source, width as usize, height as usize, factor as usize);
        return Presented {
            pixels: SharedPixelBuffer::clone_from_slice(&scaled, width * factor, height * factor),
            smooth: plan.is_none(),
            exact: plan.map(|(_, scale)| (width * scale, height * scale)),
        };
    }
    if filter != VideoFilter::Lcd || !whole {
        return Presented {
            pixels: SharedPixelBuffer::clone_from_slice(source, width, height),
            smooth,
            exact: None,
        };
    }
    // Input is validated and bounded by the libretro video callback. Three
    // subpixels per source pixel retain aspect ratio and expose a subtle grid.
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width * 3, height * 3);
    let target = pixels.make_mut_bytes();
    let stride = width as usize * 3;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let source = &source[(y * width as usize + x) * 4..][..4];
            for sy in 0..3 {
                for sx in 0..3 {
                    let offset = ((y * 3 + sy) * stride + x * 3 + sx) * 4;
                    for channel in 0..3 {
                        let stripe = if channel == sx { 255u32 } else { 232 };
                        let grid = if sy == 2 { 224u32 } else { 255 };
                        target[offset + channel] =
                            (u32::from(source[channel]) * stripe * grid / (255 * 255)) as u8;
                    }
                    target[offset + 3] = source[3];
                }
            }
        }
    }
    Presented {
        pixels,
        smooth: true,
        exact: None,
    }
}

/// sRGB GBA display profile from Pokefan531/hunterk/crashGG's public-domain
/// gba-color shader. See THIRD_PARTY_NOTICES.md for exact provenance.
/// Horizon adapts its default matrix/gamma profile for CPU RGBA frames.
pub(super) fn correct_color(rgba: &mut [u8]) {
    use std::sync::OnceLock;
    static LINEAR: OnceLock<[f32; 256]> = OnceLock::new();
    static ENCODE: OnceLock<[u8; 4097]> = OnceLock::new();
    let linear = LINEAR.get_or_init(|| std::array::from_fn(|i| (i as f32 / 255.0).powf(2.2)));
    let encode = ENCODE.get_or_init(|| {
        std::array::from_fn(|i| ((i as f32 / 4096.0).powf(1.0 / 2.2) * 255.0).round() as u8)
    });
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let [r, g, b] = [
            linear[pixel[0] as usize],
            linear[pixel[1] as usize],
            linear[pixel[2] as usize],
        ];
        for (channel, value) in [
            0.905 * r + 0.195 * g - 0.10 * b,
            0.10 * r + 0.65 * g + 0.25 * b,
            0.1575 * r + 0.1425 * g + 0.70 * b,
        ]
        .into_iter()
        .enumerate()
        {
            pixel[channel] = encode[((value * 0.91).clamp(0.0, 1.0) * 4096.0).round() as usize];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Test shorthand: the presented frame for `frame`, without correction,
    /// read back as a plain frame.
    fn apply(frame: Frame, filter: VideoFilter) -> Frame {
        let presented = present(&frame, false, filter, (0, 0), &mut Vec::new());
        Frame {
            width: presented.pixels.width(),
            height: presented.pixels.height(),
            rgba: presented.pixels.as_bytes().to_vec(),
            smooth: presented.smooth,
        }
    }
    fn sample() -> Frame {
        Frame {
            width: 2,
            height: 1,
            rgba: vec![255, 255, 255, 255, 0, 0, 0, 255],
            smooth: false,
        }
    }
    #[test]
    fn none_and_smooth_preserve_source_pixels_and_dimensions() {
        for filter in [VideoFilter::None, VideoFilter::Smooth] {
            let frame = apply(sample(), filter);
            assert_eq!((frame.width, frame.height), (2, 1));
            assert_eq!(frame.rgba, sample().rgba);
            assert_eq!(frame.smooth, filter == VideoFilter::Smooth);
        }
    }
    #[test]
    fn lcd_preserves_aspect_and_separates_subpixels_with_a_subtle_grid() {
        let frame = apply(sample(), VideoFilter::Lcd);
        assert_eq!((frame.width, frame.height), (6, 3));
        assert_eq!(frame.rgba.len(), 6 * 3 * 4);
        assert_eq!(&frame.rgba[..4], &[255, 232, 232, 255]);
        assert_eq!(&frame.rgba[4..8], &[232, 255, 232, 255]);
        assert!(frame.rgba[6 * 2 * 4] < frame.rgba[0]);
        for row in frame.rgba.as_chunks::<24>().0.iter() {
            assert!(
                row[3 * 4..]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p == &[0, 0, 0, 255])
            );
            assert!(row.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        }
    }
    #[test]
    fn color_correction_preserves_alpha_and_black_and_changes_chromatic_pixels() {
        let mut frame = Frame {
            width: 4,
            height: 1,
            rgba: vec![
                0, 0, 0, 255, 255, 255, 255, 128, 255, 0, 0, 255, 0, 255, 0, 255,
            ],
            smooth: false,
        };
        correct_color(&mut frame.rgba);
        assert_eq!(&frame.rgba[..4], &[0, 0, 0, 255]);
        assert_eq!(frame.rgba[7], 128);
        assert!((242..=247).contains(&frame.rgba[4]));
        assert!(frame.rgba[9] > 50 && frame.rgba[10] > 50);
        assert!(frame.rgba[13] < 230);
    }
    #[test]
    fn pixel_art_filter_rounds_diagonals_and_preserves_aspect_ratio() {
        let mut rgba = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                let value = if x >= y { 255 } else { 0 };
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let frame = apply(
            Frame {
                width: 8,
                height: 8,
                rgba,
                smooth: false,
            },
            VideoFilter::Xbrz,
        );
        assert_eq!((frame.width, frame.height), (48, 48));
        assert!(frame.smooth);
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 0 && p[0] < 255)
        );
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] == p[1] && p[1] == p[2])
        );
    }
    #[test]
    #[ignore = "requires Wayland and HORIZON_VIDEO_FILTER_TEST_DIR"]
    fn video_presets_render_distinct_pixels_in_real_window() {
        use slint::ComponentHandle;
        use std::{cell::Cell, rc::Rc};
        let directory =
            std::path::PathBuf::from(std::env::var_os("HORIZON_VIDEO_FILTER_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let ui = crate::AppWindow::new().unwrap();
        let height: u32 = std::env::var("HORIZON_VIDEO_FILTER_TEST_HEIGHT")
            .unwrap_or_else(|_| "720".into())
            .parse()
            .unwrap();
        ui.window()
            .set_size(slint::PhysicalSize::new(height * 16 / 9, height));
        ui.set_player_phase(2);
        ui.set_player_open(true);
        ui.set_player_has_frame(true);
        let show = |ui: &crate::AppWindow, filter| {
            let mut rgba = Vec::with_capacity(240 * 160 * 4);
            for _y in 0..160 {
                for x in 0..240 {
                    let v = if x % 2 == 0 { 255 } else { 0 };
                    rgba.extend_from_slice(&[v, v, v, 255]);
                }
            }
            let frame = apply(
                Frame {
                    width: 240,
                    height: 160,
                    rgba,
                    smooth: false,
                },
                filter,
            );
            ui.set_player_smooth(frame.smooth);
            ui.set_player_frame(slint::Image::from_rgba8(slint::SharedPixelBuffer::<
                slint::Rgba8Pixel,
            >::clone_from_slice(
                &frame.rgba,
                frame.width,
                frame.height,
            )));
        };
        show(&ui, VideoFilter::None);
        ui.show().unwrap();
        let stage = Rc::new(Cell::new(0));
        let progress = Rc::clone(&stage);
        let timer = slint::Timer::default();
        let weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(400),
            move || {
                // A large snapshot can take longer than the timer interval;
                // ignore a queued tick after requesting event-loop shutdown.
                if progress.get() >= 4 {
                    return;
                }
                let ui = weak.upgrade().unwrap();
                let filter = [
                    VideoFilter::None,
                    VideoFilter::Lcd,
                    VideoFilter::Smooth,
                    VideoFilter::Xbrz,
                ][progress.get()];
                let pixels = ui.window().take_snapshot().unwrap();
                image::save_buffer(
                    directory.join(format!("{}.png", filter.label())),
                    pixels.as_bytes(),
                    pixels.width(),
                    pixels.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
                let mut intermediate = false;
                let mut subpixel = false;
                for y in pixels.height() / 3..pixels.height() * 2 / 3 {
                    for x in pixels.width() / 3..pixels.width() * 2 / 3 {
                        let p = pixels.as_slice()[(y * pixels.width() + x) as usize];
                        intermediate |= p.r > 20 && p.r < 230;
                        subpixel |= p.r.abs_diff(p.g) > 3 || p.g.abs_diff(p.b) > 3;
                    }
                }
                match filter {
                    VideoFilter::None => {
                        assert!(!intermediate);
                        assert!(!subpixel);
                    }
                    VideoFilter::Lcd => {
                        assert!(subpixel);
                    }
                    VideoFilter::Xbrz => {
                        assert!(
                            intermediate,
                            "xBRZ output must interpolate when fitted to a fractional scale"
                        );
                        assert!(!subpixel);
                    }
                    VideoFilter::Smooth => {
                        assert!(intermediate);
                        assert!(!subpixel);
                    }
                }
                // The original 3:2 image must keep black bars in a 16:9 window.
                let edge = pixels.as_slice()[(pixels.height() / 2 * pixels.width() + 5) as usize];
                assert_eq!([edge.r, edge.g, edge.b], [0, 0, 0]);
                progress.set(progress.get() + 1);
                if progress.get() == 4 {
                    slint::quit_event_loop().unwrap();
                } else {
                    show(
                        &ui,
                        [
                            VideoFilter::None,
                            VideoFilter::Lcd,
                            VideoFilter::Smooth,
                            VideoFilter::Xbrz,
                        ][progress.get()],
                    );
                }
            },
        );
        slint::run_event_loop().unwrap();
        assert_eq!(stage.get(), 4);
    }

    #[test]
    fn xbrz_scales_by_whole_numbers_only() {
        let gba = (240, 160);
        let game_boy = (160, 144);
        // 3440×1440 ultrawide: 9× and 10× fill the height exactly.
        assert_eq!(xbrz_plan(gba, (3440, 1440)), Some((3, 9)));
        assert_eq!(xbrz_plan(game_boy, (3440, 1440)), Some((5, 10)));
        // 1920×1080: GBA fits 6×; Game Boy 7× steps down to 6×.
        assert_eq!(xbrz_plan(gba, (1920, 1080)), Some((6, 6)));
        assert_eq!(xbrz_plan(game_boy, (1920, 1080)), Some((6, 6)));
        // 4K: GBA 13× has no factor, so 12× (6× then 2×) with thin bars.
        assert_eq!(xbrz_plan(gba, (3840, 2160)), Some((6, 12)));
        assert_eq!(xbrz_plan(game_boy, (3840, 2160)), Some((5, 15)));
        assert_eq!(xbrz_plan(gba, (2560, 1440)), Some((3, 9)));
        assert_eq!(xbrz_plan(gba, (400, 300)), None);
        assert_eq!(xbrz_plan(gba, (0, 0)), None);

        let frame = Frame {
            width: 240,
            height: 160,
            rgba: [40, 80, 120, 255].repeat(240 * 160),
            smooth: false,
        };
        let shown = present(
            &frame,
            false,
            VideoFilter::Xbrz,
            (3440, 1440),
            &mut Vec::new(),
        );
        assert_eq!((shown.pixels.width(), shown.pixels.height()), (720, 480));
        assert_eq!(shown.exact, Some((2160, 1440)));
        assert!(!shown.smooth);
        let unknown = present(&frame, false, VideoFilter::Xbrz, (0, 0), &mut Vec::new());
        assert_eq!((unknown.pixels.width(), unknown.exact), (1440, None));
        assert!(unknown.smooth);
    }

    /// Old 6×-and-fit next to whole-number scaling, at a chosen window size
    /// (default 3440×1440), on an original pattern of curves and diagonals.
    #[test]
    #[ignore = "requires Wayland and HORIZON_XBRZ_SCALE_TEST_DIR"]
    fn xbrz_whole_number_scaling_is_pixel_exact_in_real_window() {
        use slint::ComponentHandle;
        let directory =
            std::path::PathBuf::from(std::env::var_os("HORIZON_XBRZ_SCALE_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let size = |name, default: u32| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        let window = (
            size("HORIZON_XBRZ_SCALE_TEST_WIDTH", 3440),
            size("HORIZON_XBRZ_SCALE_TEST_HEIGHT", 1440),
        );
        let (width, height) = (240u32, 160u32);
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let (dx, dy) = (x - 70, y - 80);
                let ring = (dx * dx + dy * dy - 2500).abs() < 260;
                let diagonal = (x - y - 120).abs() < 3 || (x + y / 2 - 230).abs() < 2;
                let colour = if ring {
                    [250, 210, 60, 255]
                } else if diagonal {
                    [70, 200, 255, 255]
                } else if (x / 16 + y / 16) % 2 == 0 {
                    [30, 40, 70, 255]
                } else {
                    [40, 60, 100, 255]
                };
                rgba.extend_from_slice(&colour);
            }
        }
        let raw = Frame {
            width,
            height,
            rgba,
            smooth: false,
        };
        let ui = crate::AppWindow::new().unwrap();
        ui.window()
            .set_size(slint::PhysicalSize::new(window.0, window.1));
        ui.set_player_phase(2);
        ui.set_player_open(true);
        ui.set_player_has_frame(true);
        ui.show().unwrap();
        let mut shots = Vec::new();
        for (name, target) in [("fit-6x", (0, 0)), ("whole-number", window)] {
            let shown = present(&raw, false, VideoFilter::Xbrz, target, &mut Vec::new());
            let (w, h) = shown.exact.unwrap_or_default();
            ui.set_player_exact_width(w as i32);
            ui.set_player_exact_height(h as i32);
            ui.set_player_smooth(shown.smooth);
            let buffer = shown.pixels.clone();
            ui.set_player_frame(slint::Image::from_rgba8(shown.pixels));
            let pixels = ui.window().take_snapshot().unwrap();
            image::save_buffer(
                directory.join(format!("{name}-{}x{}.png", window.0, window.1)),
                pixels.as_bytes(),
                pixels.width(),
                pixels.height(),
                image::ColorType::Rgba8,
            )
            .unwrap();
            // A 1:1 crop across the ring's edge, where softness shows.
            let crop = image::RgbaImage::from_fn(480, 320, |x, y| {
                let p = pixels.as_slice()[((pixels.height() / 2 - 400 + y) * pixels.width()
                    + pixels.width() / 2
                    - 900
                    + x) as usize];
                image::Rgba([p.r, p.g, p.b, p.a])
            });
            crop.save(directory.join(format!("{name}-crop.png")))
                .unwrap();
            shots.push((shown.exact, buffer, pixels));
        }
        let (exact, buffer, pixels) = &shots[1];
        let (w, h) = exact.expect("a whole-number plan for this window");
        let repeat = w / buffer.width();
        let (left, top) = ((pixels.width() - w) / 2, (pixels.height() - h) / 2);
        // Every screen pixel is exactly its xBRZ pixel: nothing resampled.
        for y in (0..h).step_by(7) {
            for x in (0..w).step_by(7) {
                let screen = pixels.as_slice()[((top + y) * pixels.width() + left + x) as usize];
                let source =
                    buffer.as_slice()[((y / repeat) * buffer.width() + x / repeat) as usize];
                assert_eq!(
                    (screen.r, screen.g, screen.b),
                    (source.r, source.g, source.b),
                    "pixel {x},{y}"
                );
            }
        }
    }
}
