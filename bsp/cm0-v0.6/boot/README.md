# V0.6 boot splash assets

`splash.png` is the 320x170 source image. The product early-splash service
writes `splash.rgb565` directly to the Linux LCD framebuffer after the ST7789
driver appears. This preserves the 64 MB VideoCore split and the standard
camera-capable Raspberry Pi firmware.

Regenerate the raw frame with FFmpeg:

```sh
ffmpeg -i splash.png -frames:v 1 -f rawvideo -pix_fmt rgb565le splash.rgb565
```

Pinned hashes:

```text
17b6b5571fd3be038992df24134d7ca88c75b22cb36e84cf2f007664096298e1  splash.png
75a53d81f5ec087536a030919698c595630d48296e07d5f5f3d04ebebf2efd57  splash.rgb565
```
