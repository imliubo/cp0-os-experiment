# Key click asset

`key-click-soft-typing-s16le.pcm` is derived from UI SFX `soft/typing.ogg` at
commit `2001f3dac2d1cf86ad99cbad5cef222c3a8b9082`:

https://github.com/romainsimon/uisfx/blob/2001f3dac2d1cf86ad99cbad5cef222c3a8b9082/packages/uisfx/sounds/soft/typing.ogg

The source Ogg SHA-256 is
`b84adcf8df711e225334c15e2f20712539d6d19a589b83b457e2714e61e06fb4`.
UI SFX audio is dedicated to the public domain under CC0-1.0; the upstream
notice is retained in `LICENSE-AUDIO`.

The production asset is exactly 512 frames of headerless, signed 16-bit
little-endian, 16 kHz mono PCM. It was generated with:

```sh
ffmpeg -i soft-typing.ogg \
  -af 'atrim=end=0.032,afade=t=out:st=0.024:d=0.008,volume=0.25,aresample=16000' \
  -ar 16000 -ac 1 -c:a pcm_s16le -f s16le \
  key-click-soft-typing-s16le.pcm
```

The derived PCM SHA-256 is
`d0cc34b9c4e4707439ce959a0592d7391496f7f7f57a01e44d65c5a14f7efefc`.
