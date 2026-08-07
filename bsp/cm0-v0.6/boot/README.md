# V0.6 boot splash assets

`splash.png` is the 320x170 source image. A bounded static initramfs helper
initializes SPI0 and ST7789 before the Linux display driver loads, then sends
`splash.rgb565` directly to panel RAM. The same asset is repainted through the
Linux framebuffer when DRM appears. This preserves the 64 MB VideoCore split
and the standard camera-capable Raspberry Pi firmware. The framebuffer path
uses a boot-scoped marker and atomic lock so the initramfs worker and systemd
fallback cannot repaint the same frame concurrently.

The official `cardputerzero_v0.6` image displays a 170x320 RGB565 BMP from a
custom `m5stack_bootscreen` VideoCore firmware before the ARM kernel. That is
earlier than an initramfs helper, but the opaque firmware ignores the product's
64 MB GPU budget and leaves Linux with only about 227 MiB. It is intentionally
rejected by the production image gate; `splash.png` remains the canonical user
asset for both bounded Linux render paths.

Regenerate the raw frame with FFmpeg:

```sh
ffmpeg -i splash.png -frames:v 1 -f rawvideo -pix_fmt rgb565le splash.rgb565
```

Pinned hashes:

```text
17b6b5571fd3be038992df24134d7ca88c75b22cb36e84cf2f007664096298e1  splash.png
75a53d81f5ec087536a030919698c595630d48296e07d5f5f3d04ebebf2efd57  splash.rgb565
```
