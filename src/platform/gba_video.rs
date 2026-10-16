//! Curated video treatments on the core worker; no shader runtime or I/O.
use super::gba_core::Frame;
use crate::domain::core_settings::VideoFilter;

pub(super) fn apply(mut frame: Frame, filter: VideoFilter) -> Frame {
    frame.smooth = matches!(
        filter,
        VideoFilter::Smooth | VideoFilter::Lcd | VideoFilter::Xbrz
    );
    if filter == VideoFilter::Xbrz {
        // The validated native frame is tiny; 3× preserves the aspect ratio.
        // Bound output allocation even if a future core reports larger frames.
        if frame.width <= 512
            && frame.height <= 512
            && frame.rgba.len() == frame.width as usize * frame.height as usize * 4
        {
            let rgba =
                xbrz::scale_rgba(&frame.rgba, frame.width as usize, frame.height as usize, 3);
            return Frame {
                width: frame.width * 3,
                height: frame.height * 3,
                rgba,
                // Fit the filtered subpixels smoothly at fractional window
                // scales; nearest-neighbour would enlarge their small steps.
                smooth: true,
            };
        }
        return frame;
    }
    if filter != VideoFilter::Lcd {
        return frame;
    }
    // Input is validated and bounded by the libretro video callback. Three
    // subpixels per source pixel retain aspect ratio and expose a subtle grid.
    let width = frame.width as usize * 3;
    let height = frame.height as usize * 3;
    let mut rgba = vec![0; width * height * 4];
    for y in 0..frame.height as usize {
        for x in 0..frame.width as usize {
            let source = &frame.rgba[(y * frame.width as usize + x) * 4..][..4];
            for sy in 0..3 {
                for sx in 0..3 {
                    let target = ((y * 3 + sy) * width + x * 3 + sx) * 4;
                    for channel in 0..3 {
                        let stripe = if channel == sx { 255u32 } else { 232 };
                        let grid = if sy == 2 { 224u32 } else { 255 };
                        rgba[target + channel] =
                            (u32::from(source[channel]) * stripe * grid / (255 * 255)) as u8;
                    }
                    rgba[target + 3] = source[3];
                }
            }
        }
    }
    Frame {
        width: width as u32,
        height: height as u32,
        rgba,
        smooth: true,
    }
}

/// sRGB GBA display profile from Pokefan531/hunterk/crashGG's public-domain
/// gba-color shader. See THIRD_PARTY_NOTICES.md for exact provenance.
/// Horizon adapts its default matrix/gamma profile for CPU RGBA frames.
pub(super) fn correct_color(frame: &mut Frame) {
    use std::sync::OnceLock;
    static LINEAR: OnceLock<[f32; 256]> = OnceLock::new();
    static ENCODE: OnceLock<[u8; 4097]> = OnceLock::new();
    let linear = LINEAR.get_or_init(|| std::array::from_fn(|i| (i as f32 / 255.0).powf(2.2)));
    let encode = ENCODE.get_or_init(|| {
        std::array::from_fn(|i| ((i as f32 / 4096.0).powf(1.0 / 2.2) * 255.0).round() as u8)
    });
    for pixel in frame.rgba.as_chunks_mut::<4>().0 {
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
        correct_color(&mut frame);
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
        assert_eq!((frame.width, frame.height), (24, 24));
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
}
