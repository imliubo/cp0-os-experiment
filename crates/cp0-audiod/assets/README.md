# Key click asset

<!-- doc-locale: en -->
> **English** | [简体中文](README.zh-CN.md)

`key-click-soft-typing-s16le.pcm` is derived from UI SFX `soft/typing.ogg` at
commit `2001f3dac2d1cf86ad99cbad5cef222c3a8b9082`:

https://github.com/romainsimon/uisfx/blob/2001f3dac2d1cf86ad99cbad5cef222c3a8b9082/packages/uisfx/sounds/soft/typing.ogg

The source Ogg SHA-256 is
`b84adcf8df711e225334c15e2f20712539d6d19a589b83b457e2714e61e06fb4`.
UI SFX audio is dedicated to the public domain under CC0-1.0; the upstream
notice is retained in `LICENSE-AUDIO`.

The retained source derivative is exactly 512 frames of headerless, signed
16-bit little-endian, 16 kHz mono PCM. It was generated with:

```sh
ffmpeg -i soft-typing.ogg \
  -af 'atrim=end=0.032,afade=t=out:st=0.024:d=0.008,volume=0.25,aresample=16000' \
  -ar 16000 -ac 1 -c:a pcm_s16le -f s16le \
  key-click-soft-typing-s16le.pcm
```

The derived PCM SHA-256 is
`d0cc34b9c4e4707439ce959a0592d7391496f7f7f57a01e44d65c5a14f7efefc`.

The production `key-click-crisp-typing-s16le.pcm` keeps the source transient
from 6 through 18 ms, applies a 1 ms attack and 4 ms fade, and raises its level
by 1.5. This removes the soft sample's audible tail while keeping the same CC0
provenance. It is exactly 192 frames, or 12 ms, and was generated with:

```sh
ffmpeg -f s16le -ar 16000 -ac 1 \
  -i key-click-soft-typing-s16le.pcm \
  -af 'atrim=start=0.006:end=0.018,asetpts=PTS-STARTPTS,afade=t=in:st=0:d=0.001,afade=t=out:st=0.008:d=0.004,volume=1.5' \
  -ar 16000 -ac 1 -c:a pcm_s16le -f s16le \
  key-click-crisp-typing-s16le.pcm
```

The production PCM SHA-256 is
`36a1701b4f097388972a9c4becbe28bdfd49487fa60736ba7eb2927a59eea821`.

Audiod keeps this asset unchanged, then appends 320 zero-valued frames in
memory before each ALSA write. The resulting 32 ms submission crosses the
period-rounded 20 ms PCM automatic-start threshold; only the original 12 ms
transient is audible.
