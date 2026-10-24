# ADR 0234 — 3DS internal resolution, screen layouts and input

Status: Accepted. 3DS playback stuttered and some inputs did two things.

Settings → Cores → Azahar → Internal resolution offers Native to 6×, stored in
`cores/azahar.json` and applied at launch through Azahar's resolution option.
Vulkan frames are read back through CPU-cached host memory when the device
offers it; uncached device-local memory made a Native frame cost 3 ms and a 4×
one about 70 ms.

Screen layouts reuse the DS ones rather than Azahar's own. Azahar keeps drawing
its default stacked frame; Horizon cuts out the 400×240 and 320×240 screens and
places them with the DS layout geometry, generalised to screens of different
shapes. Swapping and changing layout are instant, need no core option change or
new frame size, and share the DS quick-menu actions, L3 swap, per-game
`view.json` and screenshot composition. The default is Stacked, as on the
console.

Azahar's right-stick touch cursor and mouse touchscreen are turned off: the right
stick is the C-stick, the stick clicks are no longer Azahar's Home or touch (L3 is Horizon's swap), and the
pointer reaches the lower screen once, as a touch. The keyboard keys behind
stick clicks are masked for 3DS.
