# ADR 0201 — GBA colour and pixel-art treatments

Status: Accepted. The user selected xBRZ smoothing and colour correction while
retaining a small, curated set of video options.

Add Smooth pixel art to the existing Video filter choice. Use pinned xbrz-rs
0.1.0 (GPL-3.0-only), unmodified, at a fixed 3× on validated, bounded native
frames on the game worker. It is the demonstrated consumer of the new dependency;
its transitive dependencies already exist. Retain its original license text and
attribution and distribute the combined application under GPLv3 obligations.
The final renderer preserves the native aspect ratio. Following user feedback
about small steps on smoothed edges, it smoothly fits the 3× filtered output to
the viewport instead of enlarging those steps with nearest-neighbour scaling.

Colour correction is an independent, off-by-default switch. The pinned mGBA
0.10.5 core lacks the newer native option, so use a CPU adaptation of the
credited public-domain GBA sRGB profile before display filtering. Preserve raw
frames for screenshots. This approximates original screen colour response;
it does not claim per-device calibration. Attribution, source identity and the
upstream license statement are retained in third-party notices/license texts.

Both preferences use the existing atomic core-settings store and launch-time
snapshot. Old settings default to correction off and keep their existing filter.
No shader runtime, preset browser, additional core, permission or migration is
introduced. Verify actual output, saved settings, worker cost and packaging.
