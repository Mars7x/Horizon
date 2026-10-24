# ADR 0235 — 3DS GPU viewport readback

Status: Accepted. At 6× internal resolution 3DS games ran at about 50 frames a
second with constant audio gaps, while standalone Azahar held 60.

Every 2400×2880 frame (27 MB) was read back inside Azahar's frame callback,
waiting for the GPU, then copied twice more on the CPU and uploaded again. The
frame callback now only records Azahar's image. When a frame is to be shown,
Horizon shrinks each screen on the GPU to the size the layout shows it at, and
reads back only that (about 3.5 MB at 1080p). Shrinking halves the whole frame
with linear blits until each screen is within twice its target, then blits
each screen once, so every source pixel contributes. Screens shown at or above
their drawn size, and unknown window sizes, read the full frame as before.

Screenshots and full captures still read the full image on demand; queue
order and barriers keep Azahar's next frame from overwriting it first. If the
GPU can't blit the format or an image can't be allocated, the session logs one
warning and reads full frames. This mirrors the DS viewport readback
(ADR 0229) for the Vulkan core; Azahar is unmodified.
