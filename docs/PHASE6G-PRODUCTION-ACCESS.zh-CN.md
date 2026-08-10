# Phase 6G: 量产访问配置

<!-- doc-locale: zh-CN -->
> [English](PHASE6G-PRODUCTION-ACCESS.md) | **简体中文**

> `FIRST-BOOT-PROVISIONING.zh-CN.md` 和 ADR 0007 定义的首次启动 Owner 模型，是当前的
> 量产访问契约。

## 边界

正常 `product:development` 镜像故意启用了不受限制的运营商 SSH，并接受运营商选择的密码，以便唯一的一个 V0.6 板可以调试。那是一个开发产物，绝不能作为生产镜像重新分发。

`CP0_ACCESS_PROFILE=production` 创建一个独特的产物构件，默认情况下其网络维护边界是封闭的：

- 构建拒绝 `CP0_FIRST_USER_PASSWORD` 和 `CP0_SSH_PUBLIC_KEY`；
- pi-gen 只接收一个生成的 256 位临时密码，因为它的阶段合同要求一个；导出的账户被锁定并且使用 `nologin`；
- 第一个所有者没有任何 sudo、设备、网络或诊断组；
- SSH和本地/串行getty单元在Setup过程中不可用；
- 家目录中不含SSH授权目录；
- 根目录保持锁定；
- 恢复启动被根拥有的量产镜像策略锁定；
- 开发者模式默认是关闭的，但所有者可以通过物理方式启用。
- 开发者模式仅打开受限的、有符号应用部署通道；
- 默认情况下，独立的所有者 SSH Shell 选项保持关闭。

产品镜像包含无条件维护的sshd，启动分区启用标记，预设授权-key路径或首次启动热更新器。向FAT启动分区写入文件无法打开网络登录路径。SSH仅在配置完成后且开发模式或独立所有者SSH Shell设置为开启时生成和控制。所有者调度器仅将`cp0-dev`路由到受限守护进程；Bash还需要所有者SSH Shell授予的`cp0-ssh`登录组。根登录和SSH转发仍然被禁止。

NetworkManager、compositor、System Shell、appd 和能力代理服务仍然可用。此访问配置文件更改维护权限，而不更改应用程序平台或用户工作流。

## 构建

构建用于生产访问的镜像而不传递密码或SSH密钥：

```sh
CP0_ACCESS_PROFILE=production \
CP0_IMAGE_NAME=cardputerzero-os-release-candidate \
make image
make verify-image
```

artifact后缀是`-cp0-os-production.img.xz`。配置文件标记存储在lower root和种子`cp0-data`文件系统中的`/etc/cardputerzero/access-profile`，因此文件名不作为策略的信任依据。

开发和恢复构建保留其显式密码要求。
恢复镜像不能使用生产访问配置文件。提供共享密码、SSH密钥、未知访问配置文件或恢复/生产组合在访问仓库、Docker 或镜像修改开始之前会失败。

## 所有者开发和维护

Developer Mode 是面向个人量产设备的支持工作流。它要求在可信 System Shell 中物理确认，提供独立的十分钟 “Pair New Computer” 窗口，并在初次配对时验证所有者密码；授权使用 Ed25519 SSH forced-command key 和配对的开发者签名 key。它只授予安装、日志、启动、停止和卸载操作，不授予 Bash 或 root。设备支持单项和批量撤销。详见 `DEVELOPER-ACCESS.zh-CN.md`。

Owner SSH Shell 是一个单独的、明确的设置。启用它允许 Bash 作为所有者账户，但仍然不授予 sudo 权限。它不需要用于 App 开发，并且不会在开发者模式下启用。

## 恢复仪式

持续且不受限制的维护使用一个单独构建的恢复SD卡，其中包含操作员选择的一次性密码或密钥。启动该可移动映像即为物理授权仪式；移除它则会撤销访问权限。恢复映像不会自动挂载`cp0-data`，并且所有产品应用程序入口点仍然在那里被遮盖。产品映像中没有启动标记维护模式。

此设计避免在产品镜像中放置全舰队或每次发布级别的登录密钥。它不保护SD卡在离线情况下被替换，并不创建硬件根信任。dm-verity、带签名的启动元数据和A/B回滚仍然由ADR 0006管理。

## 发布门禁

挂载根文件系统验证器在账户、组、策略、服务遮罩和持久化配置标记都匹配此合同的情况下，才接受生产制品。仓库测试还会在昂贵的构建路径之前测试每种无效参数组合。

最终验收仍需在一次性介质上启动一个生产访问项，并确认默认关闭状态、受限开发者模式路径、独立所有者SSH Shell行为、根/sudo拒绝以及所有本地登录掩码。在获取其RAM支持的稳定性证据之前，它不得替换当前活动的V0.6开发镜像。
