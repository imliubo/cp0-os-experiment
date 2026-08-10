# 文档本地化

<!-- doc-locale: zh-CN -->
> [English](LOCALIZATION.md) | **简体中文**

## 语言策略

英文是仓库维护中 Markdown 文档的默认入口和规范语言。每份英文 `FILE.md` 都必须在
同一目录提供名为 `FILE.zh-CN.md` 的简体中文译文。

两个文件描述同一份契约。任何一方都不能单独保存另一种语言中不存在的额外需求、验收
结果、警告或实现状态。

## 必需页头

locale 标记和语言切换应紧跟在一级标题之后。英文文档使用：

```md
<!-- doc-locale: en -->
> **English** | [简体中文](FILE.zh-CN.md)
```

简体中文文档使用：

```md
<!-- doc-locale: zh-CN -->
> [English](FILE.md) | **简体中文**
```

如果 Skill loader 等工具要求 YAML front matter，它仍应位于文件开头。locale 标记和
语言切换放在 front matter 之后的第一个一级标题下方。

## 编辑规则

1. 在同一个任务中同步修改英文和简体中文文件。
2. 除非对应技术事实也发生变化，否则保持代码块、标识符、命令、路径、协议名、哈希、
   版本和链接目标不变。
3. 中文文档链接到其他文档时，如果对方存在 `.zh-CN.md` 版本，应优先链接中文版本。
   页头的语言切换是有意保留的英文默认文件链接。
4. 统一使用既定项目术语，尤其是：production image 译为“量产镜像”，acceptance gate
   译为“验收门禁”，immutable root 译为“不可变根文件系统”，signed integer 译为
   “有符号整数”，capability broker 译为“能力代理服务”。
5. CardputerZero、System Shell、Runtime、compositor、appd、WAMR、Wayland、Store、
   SDK、ADR、BSP、API、UID、IPC、SSH、DRM、GPIO、LoRa、WASM 和 WebAssembly 等
   产品、组件、协议及接口标识符不翻译。
6. 机器翻译只能作为草稿。合并前应对照原文检查安全约束、编号需求、否定语义、状态
   标签、单位和验收结论。
7. 必须明确保留 fail-closed 语义。请求或无效状态被拒绝，不能误译成设备关机或断电。

## 验证

可以单独运行本地化门禁，也可以通过 `make check` 一并运行：

```sh
./tests/test-document-localization.sh
make check
```

门禁会检查每份维护中的默认 Markdown 文件是否有配对译文、双方是否包含正确的 locale
标记和双向语言链接，以及默认英文文档中是否还留有未翻译的中文正文。Git 排除的生成物
和其他内容不在检查范围内。

结构检查无法证明翻译语义正确。审查时仍必须对比两种语言的修改，尤其要仔细检查安全
边界和基于真机证据得出的结论。
