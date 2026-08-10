# 计时器

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

Stopwatch 是一个权限免费的单调计时器，具有十分之一秒的精度显示和最多99小时的呈现范围。

![计时器暂停在零](assets/screenshot.png)

## 控制按钮

- Enter 或 空格: 开始或暂停。
- `R`: 重置已 elapsed 的时间。

## 在模拟器中运行

```sh
cargo run -p cp0ctl -- run examples/stopwatch \
  --duration 350 --permissions deny --keys enter,r,space \
  --output target/stopwatch.ppm
```

计时器是包含在量产镜像中的八个应用程序之一。
