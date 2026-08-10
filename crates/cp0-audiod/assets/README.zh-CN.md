# 关键点击资产

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

`key-click-soft-typing-s16le.pcm` 是从 UI SFX `soft/typing.ogg` 在提交 `2001f3dac2d1cf86ad99cbad5cef222c3a8b9082` 中派生出来的

https://github.com/romainsimon/uisfx/blob/2001f3dac2d1cf86ad99cbad5cef222c3a8b9082/packages/uisfx/sounds/soft/typing.ogg

源 Ogg SHA-256 是
`b84adcf8df711e225334c15e2f20712539d6d19a589b83b457e2714e61e06fb4`.
UI SFX 音频根据 CC0-1.0 协议进入公有领域；上游
通知保留在 `LICENSE-AUDIO` 中。

保留的源衍生品正好是512帧无头、有符号的16位小端、16 kHz单声道PCM。它是使用以下方式生成的：

```sh
ffmpeg -i soft-typing.ogg \
  -af 'atrim=end=0.032,afade=t=out:st=0.024:d=0.008,volume=0.25,aresample=16000' \
  -ar 16000 -ac 1 -c:a pcm_s16le -f s16le \
  key-click-soft-typing-s16le.pcm
```

衍生出的 PCM SHA-256 是
`d0cc34b9c4e4707439ce959a0592d7391496f7f7f57a01e44d65c5a14f7efefc`.

生产 `key-click-crisp-typing-s16le.pcm` 保持源暂态从 6 到 18 毫秒，应用 1 毫秒的攻击和 4 毫秒的淡出，并将其电平提高 1.5。这消除了软样本的可听见尾部同时保持相同的 CC0 来源。它的长度正好是 192 帧，或 12 毫秒，并且是通过以下方式生成的：

```sh
ffmpeg -f s16le -ar 16000 -ac 1 \
  -i key-click-soft-typing-s16le.pcm \
  -af 'atrim=start=0.006:end=0.018,asetpts=PTS-STARTPTS,afade=t=in:st=0:d=0.001,afade=t=out:st=0.008:d=0.004,volume=1.5' \
  -ar 16000 -ac 1 -c:a pcm_s16le -f s16le \
  key-click-crisp-typing-s16le.pcm
```

生产PCM的SHA-256是`36a1701b4f097388972a9c4becbe28bdfd49487fa60736ba7eb2927a59eea821`.

Audiod 保持该资产不变，然后在每次ALSA写入前在内存中附加320个零值帧。结果32毫秒提交跨越了周期对齐的20毫秒PCM自动启动阈值；只有原始的12毫秒瞬态是可听见的。
