# V0.6 boot splash assets

`splash.png` is the 320x170 source image. M5Stack's VideoCore boot-screen
firmware consumes the panel-native 170x320 RGB565 `splash.bmp` and rotates it
clockwise on the physical display.

Regenerate the BMP with FFmpeg:

```sh
ffmpeg -i splash.png -vf transpose=2 -c:v bmp -pix_fmt rgb565le splash.bmp
```

Pinned hashes:

```text
17b6b5571fd3be038992df24134d7ca88c75b22cb36e84cf2f007664096298e1  splash.png
dfaf289bae036e60014093cdf2705ab50d33507c38d6d197640fda99e32efc30  splash.bmp
```
