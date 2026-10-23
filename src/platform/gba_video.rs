//! Curated video treatments on the core worker; no shader runtime or I/O.
//! Each writes straight into Slint's pixel buffer, which the UI shows as is.
use super::gba_core::{Frame, Presented};
use crate::domain::core_settings::VideoFilter;
use slint::{Rgba8Pixel, SharedPixelBuffer};

/// Buffers kept between frames so presenting does not allocate.
#[derive(Default)]
pub struct Scratch {
    /// The colour-corrected copy of the raw frame.
    color: Vec<u8>,
}

/// xBRZ for a `source` frame filling a `window` (both in physical pixels):
/// the factor to run (the scale rounded up, 2–6) and the exact screen size
/// it is then resized to. None when the window cannot fit twice the source.
pub(super) fn xbrz_fit(source: (u32, u32), window: (u32, u32)) -> Option<(u32, u32, u32)> {
    if source.0 == 0 || source.1 == 0 {
        return None;
    }
    let (width, height) = (f64::from(source.0), f64::from(source.1));
    let scale = (f64::from(window.0) / width).min(f64::from(window.1) / height);
    if scale < 2.0 {
        return None;
    }
    let factor = (scale.ceil() as u32).clamp(2, 6);
    let size = |length: f64, limit: u32| ((length * scale).round() as u32).min(limit);
    Some((factor, size(width, window.0), size(height, window.1)))
}

/// A DS frame (both 256×192 screens stacked, as melonDS outputs them),
/// filtered one screen at a time so filters never blend across the seam,
/// each at the size it is shown (`targets`, physical pixels; 0×0 unknown).
pub fn present_ds(raw: &Frame, filter: VideoFilter, targets: [(u32, u32); 2]) -> Presented {
    // Two 4:3 screens stacked: 256×384 natively, a multiple when upscaled.
    let (width, height) = (raw.width, raw.height / 2);
    if raw.width == 0
        || raw.width * 3 != height * 4
        || !raw.height.is_multiple_of(2)
        || raw.rgba.len() != (width * height * 8) as usize
    {
        // Not the expected two screens: show it whole rather than guess.
        return present_source(&raw.rgba, (raw.width, raw.height), filter, (0, 0));
    }
    // xBRZ reconstructs native pixel art. Keep higher-resolution 3D intact;
    // Settings explicitly describes this choice as native-only.
    let filter = if width > 256 && filter == VideoFilter::Xbrz {
        VideoFilter::Smooth
    } else {
        filter
    };
    if filter == VideoFilter::None
        && let Some(sizes) = ds_viewport_sizes((raw.width, raw.height), targets)
    {
        return ds_viewport_pixels(raw, sizes);
    }
    if matches!(filter, VideoFilter::None | VideoFilter::Smooth) {
        // No per-screen treatment is needed: one copy keeps both screens in
        // their original positions. At 8× this avoids two temporary 12 MiB
        // images and the extra 24 MiB assembly copy for every shown frame.
        return Presented {
            pixels: SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &raw.rgba, raw.width, raw.height,
            ),
            smooth: filter == VideoFilter::Smooth,
            exact: None,
            screens: Some([(width, height); 2]),
        };
    }
    let half = (width * height * 4) as usize;
    let [top, bottom] = [0, 1].map(|index| {
        let source = &raw.rgba[index * half..(index + 1) * half];
        if filter == VideoFilter::Lcd && width > 256 && width.is_multiple_of(256) {
            return lcd_upscaled_ds(source, width, height);
        }
        present_source(source, (width, height), filter, targets[index])
    });
    let sizes = [&top, &bottom].map(|screen| (screen.pixels.width(), screen.pixels.height()));
    let width = sizes[0].0.max(sizes[1].0);
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, sizes[0].1 + sizes[1].1);
    let stride = width as usize * 4;
    let target = pixels.make_mut_bytes();
    let mut row = 0;
    for screen in [&top, &bottom] {
        let line = screen.pixels.width() as usize * 4;
        for source in screen.pixels.as_bytes().chunks_exact(line) {
            target[row * stride..row * stride + line].copy_from_slice(source);
            row += 1;
        }
    }
    Presented {
        pixels,
        smooth: top.smooth,
        exact: None,
        screens: Some(sizes),
    }
}

/// Exact screen sizes for viewport-only nearest presentation. Capture/rewind
/// frames retain the core's full dimensions. Unknown/enlarging targets fall back.
pub(super) fn ds_viewport_sizes(
    (width, stacked_height): (u32, u32),
    targets: [(u32, u32); 2],
) -> Option<[(u32, u32); 2]> {
    let height = stacked_height / 2;
    if width <= 256
        || width * 3 != height * 4
        || !stacked_height.is_multiple_of(2)
        || !targets
            .iter()
            .all(|&(w, h)| w > 0 && h > 0 && w <= width && h <= height)
        || !targets.iter().any(|&(w, h)| w < width || h < height)
    {
        return None;
    }
    Some(targets.map(|(w, h)| {
        let scale = (f64::from(w) / f64::from(width)).min(f64::from(h) / f64::from(height));
        (
            (f64::from(width) * scale).round().max(1.0) as u32,
            (f64::from(height) * scale).round().max(1.0) as u32,
        )
    }))
}

fn ds_viewport_pixels(raw: &Frame, sizes: [(u32, u32); 2]) -> Presented {
    let (width, height) = (raw.width, raw.height / 2);
    let stride = sizes[0].0.max(sizes[1].0);
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(stride, sizes[0].1 + sizes[1].1);
    let output = pixels.make_mut_bytes();
    let mut first_row = 0;
    for (screen, (w, h)) in sizes.into_iter().enumerate() {
        let columns: Vec<_> = (0..w)
            .map(|x| (u64::from(x * 2 + 1) * u64::from(width) / u64::from(w * 2)) as usize * 4)
            .collect();
        for y in 0..h {
            let source_y = (u64::from(y * 2 + 1) * u64::from(height) / u64::from(h * 2)) as u32;
            let source = ((screen as u32 * height + source_y) * width) as usize * 4;
            let target = ((first_row + y) * stride) as usize * 4;
            let row = &mut output[target..target + w as usize * 4];
            for (pixel, column) in row.as_chunks_mut::<4>().0.iter_mut().zip(&columns) {
                pixel.copy_from_slice(&raw.rgba[source + column..source + column + 4]);
            }
        }
        first_row += h;
    }
    Presented {
        pixels,
        smooth: false,
        exact: None,
        screens: Some(sizes),
    }
}

/// Apply the native LCD pattern without downsampling higher-resolution 3D
/// or allocating a 3× intermediate. Each source pixel averages the coverage
/// of the native RGB thirds, so non-multiples of three stay colour-balanced.
fn lcd_upscaled_ds(source: &[u8], width: u32, height: u32) -> Presented {
    let scale = width / 256;
    let overlap = |pixel: u32, band: u32| {
        ((pixel + 1) * 3)
            .min((band + 1) * scale)
            .saturating_sub((pixel * 3).max(band * scale))
    };
    let stripes: Vec<[u32; 3]> = (0..scale)
        .map(|x| std::array::from_fn(|channel| 232 * 3 + 23 * overlap(x, channel as u32)))
        .collect();
    let grid: Vec<u32> = (0..scale).map(|y| 255 * 3 - 31 * overlap(y, 2)).collect();
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    for (index, (target, source)) in pixels
        .make_mut_bytes()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(source.as_chunks::<4>().0)
        .enumerate()
    {
        let x = index % width as usize % scale as usize;
        let y = index / width as usize % scale as usize;
        for channel in 0..3 {
            target[channel] = (u32::from(source[channel]) * stripes[x][channel] * grid[y]
                / (255 * 255 * 9)) as u8;
        }
        target[3] = source[3];
    }
    Presented {
        pixels,
        smooth: true,
        exact: None,
        screens: None,
    }
}

/// A DS screenshot as the player shows it: the two screens of the core's
/// stacked frame in `layout`, in `swapped` order, at full resolution. Large
/// and small at native resolution is drawn at 3× so the small screen keeps
/// whole pixels (1024×576); upscaled frames average the small screen down.
/// Anything not two stacked 4:3 screens is returned as it is.
pub fn ds_screenshot(
    raw: &Frame,
    layout: crate::domain::core_settings::DsLayout,
    swapped: bool,
) -> Frame {
    use crate::domain::core_settings::DsLayout;
    let (width, height) = (raw.width as usize, raw.height as usize / 2);
    if width == 0
        || width * 3 != height * 4
        || !raw.height.is_multiple_of(2)
        || raw.rgba.len() != width * height * 8
    {
        return raw.clone();
    }
    let half = width * height * 4;
    let mut screens = [&raw.rgba[..half], &raw.rgba[half..]];
    if swapped {
        screens.swap(0, 1);
    }
    // Copy `source` (w×h) scaled by whole `up` or averaged down by `down`
    // into `canvas` (stride `cw` pixels) at (x, y).
    let place = |canvas: &mut [u8],
                 cw: usize,
                 source: &[u8],
                 (x, y): (usize, usize),
                 up: usize,
                 down: usize| {
        let (sw, sh) = (width * up / down, height * up / down);
        for row in 0..sh {
            for column in 0..sw {
                let mut sum = [0u32; 4];
                for dy in 0..down {
                    for dx in 0..down {
                        let (sx, sy) = ((column * down + dx) / up, (row * down + dy) / up);
                        let p = &source[(sy * width + sx) * 4..][..4];
                        (0..4).for_each(|c| sum[c] += u32::from(p[c]));
                    }
                }
                let area = (down * down) as u32;
                let target = ((y + row) * cw + x + column) * 4;
                for c in 0..4 {
                    canvas[target + c] = (sum[c] / area) as u8;
                }
            }
        }
    };
    let (cw, ch, rgba) = match layout {
        DsLayout::SideBySide => {
            let (cw, ch) = (width * 2, height);
            let mut canvas = vec![0; cw * ch * 4];
            place(&mut canvas, cw, screens[0], (0, 0), 1, 1);
            place(&mut canvas, cw, screens[1], (width, 0), 1, 1);
            (cw, ch, canvas)
        }
        DsLayout::Stacked => {
            let mut rgba = Vec::with_capacity(half * 2);
            rgba.extend_from_slice(screens[0]);
            rgba.extend_from_slice(screens[1]);
            (width, height * 2, rgba)
        }
        DsLayout::LargeAndSmall => {
            // The small screen is a third the size, bottom-aligned beside.
            let up = if width == 256 { 3 } else { 1 };
            let (main_w, main_h) = (width * up, height * up);
            let (small_w, small_h) = (main_w / 3, main_h / 3);
            let (cw, ch) = (main_w + small_w, main_h);
            let mut canvas = vec![0; cw * ch * 4];
            for alpha in canvas.iter_mut().skip(3).step_by(4) {
                *alpha = 255;
            }
            place(&mut canvas, cw, screens[0], (0, 0), up, 1);
            place(
                &mut canvas,
                cw,
                screens[1],
                (main_w, main_h - small_h),
                up,
                3,
            );
            (cw, ch, canvas)
        }
    };
    Frame {
        width: cw as u32,
        height: ch as u32,
        rgba,
        smooth: raw.smooth,
    }
}

/// The blurred picture shown while a game loads or saves on exit: an
/// upscaled frame is first averaged down to native size (blurring needs no
/// detail, and it keeps this a few milliseconds).
pub fn loading_picture(frame: &Frame, ds: bool) -> Frame {
    let factor = (frame.width / if ds { 256 } else { 240 }).max(1);
    let native = if factor > 1
        && frame.width.is_multiple_of(factor)
        && frame.height.is_multiple_of(factor)
        && frame.rgba.len() == (frame.width * frame.height * 4) as usize
    {
        let (width, height) = (frame.width / factor, frame.height / factor);
        let (stride, factor) = (frame.width as usize * 4, factor as usize);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = (index % width as usize, index / width as usize);
            let mut sum = [0u32; 3];
            for dy in 0..factor {
                let row = &frame.rgba[(y * factor + dy) * stride + x * factor * 4..][..factor * 4];
                for source in row.as_chunks::<4>().0 {
                    (0..3).for_each(|c| sum[c] += u32::from(source[c]));
                }
            }
            let area = (factor * factor) as u32;
            *pixel = [
                (sum[0] / area) as u8,
                (sum[1] / area) as u8,
                (sum[2] / area) as u8,
                255,
            ];
        }
        Frame {
            width,
            height,
            rgba,
            smooth: true,
        }
    } else {
        frame.clone()
    };
    blurred(&native, if ds { 2 } else { 1 })
}

/// A soft blur of `frame` for the loading picture: three box blurs (close
/// to a Gaussian) of a radius proportional to its width. `bands` horizontal
/// strips are blurred apart, so the DS's two screens don't bleed together.
pub fn blurred(frame: &Frame, bands: u32) -> Frame {
    let (width, height) = (frame.width as usize, frame.height as usize);
    let bands = bands.max(1) as usize;
    if width == 0 || height == 0 || frame.rgba.len() != width * height * 4 || height % bands != 0 {
        return frame.clone();
    }
    let radius = (width / 48).max(2);
    let mut pixels: Vec<[f32; 3]> = frame
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])])
        .collect();
    let band = height / bands;
    let mut line = Vec::new();
    // One box pass along a line of `count` pixels, `step` apart from `start`.
    let mut pass = |pixels: &mut [[f32; 3]], start: usize, step: usize, count: usize| {
        line.clear();
        line.extend((0..count).map(|i| pixels[start + i * step]));
        let mut sum = [0.0f32; 3];
        let at = |i: isize| line[i.clamp(0, count as isize - 1) as usize];
        for i in -(radius as isize)..=radius as isize {
            let p = at(i);
            (0..3).for_each(|c| sum[c] += p[c]);
        }
        let scale = 1.0 / (radius * 2 + 1) as f32;
        for i in 0..count {
            pixels[start + i * step] = sum.map(|v| v * scale);
            let (leaving, entering) = (
                at(i as isize - radius as isize),
                at(i as isize + radius as isize + 1),
            );
            (0..3).for_each(|c| sum[c] += entering[c] - leaving[c]);
        }
    };
    for _ in 0..3 {
        for y in 0..height {
            pass(&mut pixels, y * width, 1, width);
        }
        for b in 0..bands {
            for x in 0..width {
                pass(&mut pixels, b * band * width + x, width, band);
            }
        }
    }
    let rgba = pixels
        .iter()
        .flat_map(|p| [p[0] as u8, p[1] as u8, p[2] as u8, 255])
        .collect();
    Frame {
        width: frame.width,
        height: frame.height,
        rgba,
        smooth: true,
    }
}

/// Fixed-point weight total for area resampling.
const ONE: u32 = 1 << 14;

/// The threads one frame's filtering is split across.
fn bands(rows: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(4)
        .min(rows / 16)
        .max(1)
}

/// Rows above and below each band, so xBRZ sees the same neighbourhood as
/// in one pass and the bands join without seams.
const XBRZ_MARGIN: usize = 4;

/// xBRZ in horizontal bands on several threads; identical to one pass.
fn xbrz_scale(source: &[u8], width: usize, height: usize, factor: usize) -> Vec<u8> {
    let count = bands(height);
    if count == 1 {
        return xbrz::scale_rgba(source, width, height, factor);
    }
    let mut output = vec![0; width * height * factor * factor * 4];
    let band = height.div_ceil(count);
    let out_stride = width * factor * 4;
    std::thread::scope(|scope| {
        for (index, target) in output.chunks_mut(band * factor * out_stride).enumerate() {
            scope.spawn(move || {
                let first = index * band;
                let last = (first + band).min(height);
                let (from, to) = (
                    first.saturating_sub(XBRZ_MARGIN),
                    (last + XBRZ_MARGIN).min(height),
                );
                let scaled = xbrz::scale_rgba(
                    &source[from * width * 4..to * width * 4],
                    width,
                    to - from,
                    factor,
                );
                let skip = (first - from) * factor * out_stride;
                target.copy_from_slice(&scaled[skip..skip + target.len()]);
            });
        }
    });
    output
}

/// Per output pixel along one axis: the first source pixel it covers and the
/// weights of the (at most three) pixels it covers, summing to ONE. Output
/// is at least half the source's size here, so three always suffice.
fn area_taps(source: usize, target: usize) -> Vec<(usize, [u32; 3], usize)> {
    // Output i covers [i * source, (i + 1) * source) and source pixel j
    // covers [j * target, (j + 1) * target), in units of 1 / (source * target).
    (0..target)
        .map(|i| {
            let (from, to) = (i * source, (i + 1) * source);
            let first = from / target;
            let last = ((to - 1) / target).min(first + 2);
            let mut weights = [0; 3];
            for (slot, j) in (first..=last).enumerate() {
                let overlap = to.min((j + 1) * target) - from.max(j * target);
                weights[slot] = (overlap as u64 * u64::from(ONE) / source as u64) as u32;
            }
            let count = last - first + 1;
            let total: u32 = weights.iter().sum();
            weights[count - 1] += ONE - total;
            (first, weights, count)
        })
        .collect()
}

/// Area-weighted resize: each output pixel averages exactly the source it
/// covers. Enlarging xBRZ output by a fraction blends at most one pixel
/// across each edge, so edges stay sharp and flat areas are untouched.
/// Pixels and rows covered by one source pixel are plain copies.
fn resample_area(
    source: &[u8],
    (width, height): (usize, usize),
    (target_width, target_height): (usize, usize),
    target: &mut [u8],
) {
    let columns = area_taps(width, target_width);
    let rows = area_taps(height, target_height);
    let (stride, target_stride) = (width * 4, target_width * 4);
    let band = target_height.div_ceil(bands(target_height));
    let (columns, rows) = (&columns, &rows);
    std::thread::scope(|scope| {
        for (index, target) in target.chunks_mut(band * target_stride).enumerate() {
            scope.spawn(move || {
                let mut blended = vec![0u8; stride];
                for (offset, out) in target.chunks_exact_mut(target_stride).enumerate() {
                    let (first, weights, count) = rows[index * band + offset];
                    let line: &[u8] = if count == 1 {
                        &source[first * stride..][..stride]
                    } else {
                        for (x, value) in blended.iter_mut().enumerate() {
                            let mut sum = ONE / 2;
                            for tap in 0..count {
                                sum += u32::from(source[(first + tap) * stride + x]) * weights[tap];
                            }
                            *value = (sum / ONE) as u8;
                        }
                        &blended
                    };
                    let pixels = line.as_chunks::<4>().0;
                    for (&(first, weights, count), pixel) in
                        columns.iter().zip(out.as_chunks_mut::<4>().0)
                    {
                        if count == 1 {
                            *pixel = pixels[first];
                            continue;
                        }
                        let mut sum = [ONE / 2; 4];
                        for tap in 0..count {
                            for channel in 0..4 {
                                sum[channel] +=
                                    u32::from(pixels[first + tap][channel]) * weights[tap];
                            }
                        }
                        *pixel = sum.map(|value| (value / ONE) as u8);
                    }
                }
            });
        }
    });
}

/// The frame to show for `raw` (left untouched: screenshots use it).
/// `window` is the player's size in physical pixels (0×0 when unknown).
pub fn present(
    raw: &Frame,
    color_correction: bool,
    filter: VideoFilter,
    window: (u32, u32),
    scratch: &mut Scratch,
) -> Presented {
    let (width, height) = (raw.width, raw.height);
    let mut color = std::mem::take(&mut scratch.color);
    let source: &[u8] = if color_correction {
        color.clear();
        color.extend_from_slice(&raw.rgba);
        correct_color(&mut color);
        &color
    } else {
        &raw.rgba
    };
    let presented = present_source(source, (width, height), filter, window);
    scratch.color = color;
    presented
}

fn present_source(
    source: &[u8],
    (width, height): (u32, u32),
    filter: VideoFilter,
    window: (u32, u32),
) -> Presented {
    let smooth = matches!(
        filter,
        VideoFilter::Smooth | VideoFilter::Lcd | VideoFilter::Xbrz
    );
    let whole = source.len() == width as usize * height as usize * 4;
    // Bound output allocation even if a future core reports larger frames.
    if filter == VideoFilter::Xbrz && whole && width <= 512 && height <= 512 {
        let fit = xbrz_fit((width, height), window);
        // Unknown or tiny window: reconstruct at 6× and let the GPU fit it.
        let factor = fit.map_or(6, |(factor, ..)| factor);
        let mut scaled = xbrz_scale(source, width as usize, height as usize, factor as usize);
        // xBRZ rounds the frame's own corners against transparency outside
        // it; game frames are opaque.
        for pixel in scaled.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        let (scaled_width, scaled_height) = (width * factor, height * factor);
        let Some((_, target_width, target_height)) = fit else {
            return Presented {
                pixels: SharedPixelBuffer::clone_from_slice(&scaled, scaled_width, scaled_height),
                smooth: true,
                exact: None,
                screens: None,
            };
        };
        // Shown 1:1 at the screen's own size, so the GPU never resamples.
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(target_width, target_height);
        if (target_width, target_height) == (scaled_width, scaled_height) {
            pixels.make_mut_bytes().copy_from_slice(&scaled);
        } else {
            resample_area(
                &scaled,
                (scaled_width as usize, scaled_height as usize),
                (target_width as usize, target_height as usize),
                pixels.make_mut_bytes(),
            );
        }
        return Presented {
            pixels,
            smooth: false,
            exact: Some((target_width, target_height)),
            screens: None,
        };
    }
    if filter != VideoFilter::Lcd || !whole {
        return Presented {
            pixels: SharedPixelBuffer::clone_from_slice(source, width, height),
            smooth,
            exact: None,
            screens: None,
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
        screens: None,
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

    #[test]
    fn upscaled_ds_none_uploads_viewport_pixels_without_losing_screen_bounds() {
        let mut raw = Frame {
            width: 2048,
            height: 3072,
            rgba: [20, 30, 40, 255].repeat(2048 * 3072),
            smooth: false,
        };
        for (x, y, color) in [
            (2, 2, [255, 0, 0, 255]),
            (2046, 1534, [0, 255, 0, 255]),
            (4, 1540, [0, 0, 255, 255]),
        ] {
            let offset = (y * 2048 + x) * 4;
            raw.rgba[offset..offset + 4].copy_from_slice(&color);
        }
        let shown = present_ds(&raw, VideoFilter::None, [(512, 384), (256, 192)]);
        assert_eq!(shown.screens, Some([(512, 384), (256, 192)]));
        assert_eq!((shown.pixels.width(), shown.pixels.height()), (512, 576));
        assert!(!shown.smooth);
        let bytes = shown.pixels.as_bytes();
        assert_eq!(&bytes[..4], &[255, 0, 0, 255]);
        let top_end = (383 * 512 + 511) * 4;
        assert_eq!(&bytes[top_end..top_end + 4], &[0, 255, 0, 255]);
        let bottom = 384 * 512 * 4;
        assert_eq!(&bytes[bottom..bottom + 4], &[0, 0, 255, 255]);
        assert_eq!(
            (raw.width, raw.height, raw.rgba.len()),
            (2048, 3072, 2048 * 3072 * 4)
        );
        // Unknown viewport and smooth filtering retain the full source image.
        for (filter, targets) in [
            (VideoFilter::None, [(0, 0); 2]),
            (VideoFilter::Smooth, [(512, 384), (256, 192)]),
        ] {
            let shown = present_ds(&raw, filter, targets);
            assert_eq!(shown.pixels.as_bytes(), raw.rgba);
        }
    }

    #[test]
    fn ds_none_and_smooth_keep_every_pixel_and_both_screen_bounds() {
        for scale in 1..=8 {
            let (width, height) = (256 * scale, 192 * scale);
            let mut rgba = [210, 0, 0, 191].repeat((width * height) as usize);
            rgba.extend([0, 0, 210, 223].repeat((width * height) as usize));
            rgba[4..8].copy_from_slice(&[0, 210, 0, 77]);
            let raw = Frame {
                width,
                height: height * 2,
                rgba,
                smooth: false,
            };
            for filter in [VideoFilter::None, VideoFilter::Smooth] {
                let presented = present_ds(&raw, filter, [(0, 0); 2]);
                assert_eq!(presented.pixels.as_bytes(), raw.rgba);
                assert_eq!(
                    (presented.pixels.width(), presented.pixels.height()),
                    (width, height * 2)
                );
                assert_eq!(presented.screens, Some([(width, height); 2]));
                assert_eq!(presented.smooth, filter == VideoFilter::Smooth);
            }
        }
    }

    #[test]
    fn upscaled_ds_lcd_keeps_detail_and_separates_screens_at_every_scale() {
        for scale in 2..=8 {
            let (width, height) = (256 * scale, 192 * scale);
            let mut rgba = [210u8, 0, 0, 191].repeat((width * height) as usize);
            rgba.extend([0u8, 0, 210, 223].repeat((width * height) as usize));
            // A one-high-resolution-pixel feature must not be averaged away.
            rgba[4..8].copy_from_slice(&[0, 210, 0, 191]);
            let raw = Frame {
                width,
                height: height * 2,
                rgba,
                smooth: false,
            };
            let lcd = present_ds(&raw, VideoFilter::Lcd, [(0, 0); 2]);
            assert_eq!(
                (lcd.pixels.width(), lcd.pixels.height()),
                (width, height * 2)
            );
            assert_eq!(lcd.screens, Some([(width, height); 2]));
            let pixels = lcd.pixels.as_bytes();
            assert!(pixels[0] > 0 && pixels[1] == 0 && pixels[2] == 0);
            assert!(pixels[4] == 0 && pixels[5] > 0 && pixels[6] == 0);
            let bottom = (width * height * 4) as usize;
            assert!(pixels[bottom] == 0 && pixels[bottom + 2] > 0);
            assert_eq!((pixels[3], pixels[bottom + 3]), (191, 223));
            assert_ne!(pixels, raw.rgba.as_slice());
            let smooth = present_ds(&raw, VideoFilter::Smooth, [(0, 0); 2]);
            let pixel_art = present_ds(&raw, VideoFilter::Xbrz, [(0, 0); 2]);
            assert_eq!(smooth.pixels.as_bytes(), raw.rgba.as_slice());
            assert_eq!(pixel_art.pixels.as_bytes(), smooth.pixels.as_bytes());
        }
    }

    #[test]
    fn three_times_ds_lcd_matches_the_native_pattern_without_extra_scaling() {
        let native = Frame {
            width: 256,
            height: 384,
            rgba: [180u8, 120, 60, 200].repeat(256 * 384),
            smooth: false,
        };
        let upscaled = Frame {
            width: 768,
            height: 1152,
            rgba: [180u8, 120, 60, 200].repeat(768 * 1152),
            smooth: false,
        };
        let expected = present_ds(&native, VideoFilter::Lcd, [(0, 0); 2]);
        let actual = present_ds(&upscaled, VideoFilter::Lcd, [(0, 0); 2]);
        assert_eq!(actual.pixels.as_bytes(), expected.pixels.as_bytes());
    }

    #[test]
    fn upscaled_lcd_subpixel_coverage_stays_colour_balanced() {
        for scale in 2..=8 {
            let width = 256 * scale;
            let source = [255u8; 4].repeat((width * scale) as usize);
            let lcd = lcd_upscaled_ds(&source, width, scale);
            let mut sum = [0i64; 3];
            for row in lcd.pixels.as_bytes().chunks_exact((width * 4) as usize) {
                for pixel in row[..(scale * 4) as usize].as_chunks::<4>().0 {
                    for channel in 0..3 {
                        sum[channel] += i64::from(pixel[channel]);
                    }
                }
            }
            let difference = sum.iter().max().unwrap() - sum.iter().min().unwrap();
            assert!(difference <= i64::from(scale * scale), "{scale}×: {sum:?}");
        }
    }

    #[test]
    fn ds_screenshots_follow_the_layout_and_swap() {
        use crate::domain::core_settings::DsLayout;
        // Top screen red, bottom blue, at native size.
        let mut rgba = [255u8, 0, 0, 255].repeat(256 * 192);
        rgba.extend([0u8, 0, 255, 255].repeat(256 * 192));
        let raw = Frame {
            width: 256,
            height: 384,
            rgba,
            smooth: false,
        };
        let pixel = |frame: &Frame, x: u32, y: u32| {
            frame.rgba[((y * frame.width + x) * 4) as usize..][..3].to_vec()
        };
        let side = ds_screenshot(&raw, DsLayout::SideBySide, false);
        assert_eq!((side.width, side.height), (512, 192));
        assert_eq!(pixel(&side, 0, 0), [255, 0, 0]);
        assert_eq!(pixel(&side, 300, 0), [0, 0, 255]);
        let swapped = ds_screenshot(&raw, DsLayout::SideBySide, true);
        assert_eq!(pixel(&swapped, 0, 0), [0, 0, 255]);
        let stacked = ds_screenshot(&raw, DsLayout::Stacked, true);
        assert_eq!((stacked.width, stacked.height), (256, 384));
        assert_eq!(pixel(&stacked, 0, 0), [0, 0, 255]);
        let large = ds_screenshot(&raw, DsLayout::LargeAndSmall, false);
        assert_eq!((large.width, large.height), (1024, 576));
        assert_eq!(pixel(&large, 10, 10), [255, 0, 0]);
        // The small screen, bottom right; black above it.
        assert_eq!(pixel(&large, 1000, 570), [0, 0, 255]);
        assert_eq!(pixel(&large, 1000, 10), [0, 0, 0]);
    }

    #[test]
    fn upscaled_ds_large_small_captures_do_not_triple_high_resolution_frames() {
        use crate::domain::core_settings::DsLayout;
        for scale in 2..=8 {
            let (width, height) = (256 * scale, 192 * scale);
            let raw = Frame {
                width,
                height: height * 2,
                rgba: [30u8, 60, 90, 255].repeat((width * height * 2) as usize),
                smooth: true,
            };
            let capture = ds_screenshot(&raw, DsLayout::LargeAndSmall, false);
            assert_eq!((capture.width, capture.height), (width + width / 3, height));
        }
    }
    /// Test shorthand: the presented frame for `frame`, without correction,
    /// read back as a plain frame.
    fn apply(frame: Frame, filter: VideoFilter) -> Frame {
        let presented = present(&frame, false, filter, (0, 0), &mut Scratch::default());
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
    fn xbrz_fills_the_screen_from_a_factor_of_at_least_the_scale() {
        let gba = (240, 160);
        let game_boy = (160, 144);
        // 3440×1440 ultrawide: 9× and 10×, resized down from 6×.
        assert_eq!(xbrz_fit(gba, (3440, 1440)), Some((6, 2160, 1440)));
        assert_eq!(xbrz_fit(game_boy, (3440, 1440)), Some((6, 1600, 1440)));
        // 4K fills the height too (13.5×), no bars.
        assert_eq!(xbrz_fit(gba, (3840, 2160)), Some((6, 3240, 2160)));
        // Small windows run a factor just above their scale and shrink it.
        assert_eq!(xbrz_fit(gba, (1280, 720)), Some((5, 1080, 720)));
        assert_eq!(xbrz_fit(gba, (400, 300)), None);
        assert_eq!(xbrz_fit(gba, (0, 0)), None);

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
            &mut Scratch::default(),
        );
        assert_eq!((shown.pixels.width(), shown.pixels.height()), (2160, 1440));
        assert_eq!(shown.exact, Some((2160, 1440)));
        assert!(!shown.smooth);
        // Flat colour stays exactly flat through the resize.
        assert!(
            shown
                .pixels
                .as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [40, 80, 120, 255])
        );
        let unknown = present(
            &frame,
            false,
            VideoFilter::Xbrz,
            (0, 0),
            &mut Scratch::default(),
        );
        assert_eq!((unknown.pixels.width(), unknown.exact), (1440, None));
        assert!(unknown.smooth);
    }

    /// Time per presented frame; run with --release.
    #[test]
    #[ignore = "timing report; run with --release"]
    fn xbrz_screen_size_cost() {
        let mut rgba = Vec::with_capacity(240 * 160 * 4);
        for y in 0..160u32 {
            for x in 0..240u32 {
                let tile = ((x / 8) ^ (y / 8)) & 3;
                rgba.extend_from_slice(&[(tile * 60) as u8, (y * 255 / 160) as u8, 90, 255]);
            }
        }
        let gba = Frame {
            width: 240,
            height: 160,
            rgba,
            smooth: false,
        };
        let mut scratch = Scratch::default();
        for window in [(1920, 1080), (3440, 1440), (3840, 2160)] {
            present(&gba, false, VideoFilter::Xbrz, window, &mut scratch);
            let start = std::time::Instant::now();
            for _ in 0..30 {
                std::hint::black_box(present(
                    &gba,
                    false,
                    VideoFilter::Xbrz,
                    window,
                    &mut scratch,
                ));
            }
            eprintln!(
                "TIMING {window:?}: {:.2} ms",
                start.elapsed().as_secs_f64() * 1000.0 / 30.0
            );
        }
    }

    #[test]
    fn area_resize_keeps_edges_within_one_pixel() {
        // Black then white, 2 pixels resized to 3: only the pixel straddling
        // the edge blends.
        let source = [[0u8, 0, 0, 255], [255, 255, 255, 255]].concat();
        let mut target = vec![0; 3 * 4];
        resample_area(&source, (2, 1), (3, 1), &mut target);
        let reds: Vec<u8> = target.as_chunks::<4>().0.iter().map(|p| p[0]).collect();
        assert_eq!(reds, vec![0, 128, 255]);
        assert!(target.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        for taps in [
            area_taps(1440, 2160),
            area_taps(1200, 1080),
            area_taps(4, 6),
        ] {
            assert!(taps.iter().all(|(_, w, _)| w.iter().sum::<u32>() == ONE));
        }
    }

    #[test]
    fn banded_xbrz_matches_one_pass() {
        let (width, height) = (240usize, 160usize);
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = (x as i32 - 120, y as i32 - 80);
                let ring = (dx * dx + dy * dy - 3000).abs() < 300;
                let stripe = (x + y * 3) % 23 < 4;
                let v = if ring {
                    250
                } else if stripe {
                    120
                } else {
                    ((x ^ y) & 31) as u8
                };
                rgba.extend_from_slice(&[v, 255 - v, (x * 255 / width) as u8, 255]);
            }
        }
        for factor in [2, 5, 6] {
            assert!(
                xbrz_scale(&rgba, width, height, factor)
                    == xbrz::scale_rgba(&rgba, width, height, factor),
                "factor {factor}"
            );
        }
    }

    /// Old 6×-and-GPU-fit next to the screen-size resize, at a chosen window size
    /// (default 3440×1440), on an original pattern of curves and diagonals.
    #[test]
    #[ignore = "requires Wayland and HORIZON_XBRZ_SCALE_TEST_DIR"]
    fn xbrz_screen_size_output_is_shown_pixel_exact_in_real_window() {
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
        for (name, target) in [("fit-6x", (0, 0)), ("screen-size", window)] {
            let shown = present(
                &raw,
                false,
                VideoFilter::Xbrz,
                target,
                &mut Scratch::default(),
            );
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
        let (w, h) = exact.expect("a screen-size plan for this window");
        let repeat = w / buffer.width();
        let (left, top) = ((pixels.width() - w) / 2, (pixels.height() - h) / 2);
        // Every screen pixel is exactly the presented pixel: no GPU resampling.
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
