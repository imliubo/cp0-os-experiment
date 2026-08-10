# Phase 2E：已安装应用的 Launcher

<!-- doc-locale: zh-CN -->
> [English](PHASE2E-LAUNCHER-LIFECYCLE.md) | **简体中文**

## 生命周期模型

System Shell 不再把已映射的 Wayland 表面视为已安装应用。它通过查询 appd 的认证控制套接字获取规范 manifest 元数据，并独立于临时 compositor token 维护该目录。

私有应用摘要现在包括：

- 安装的清单中的应用 ID、显示名称和版本；
- 标准或沉浸显示策略；
- 当前 appd/systemd 运行状态。

响应保持在8 KiB协议帧下方，每页请求八个条目。Shell最多接受32个条目，如果存在更多条目则显示`32+`标记，并在320x170的显示上滚动四行。这是启动器UI限制，而不是权限边界；appd仍然是唯一来源。

选择一个停止的应用程序会发送一个 appd 开始命令，标记该行状态为 STARTING，并记录标准应用程序 ID 为待激活状态。可信的应用运行时后来通过 compositor 映射其界面。Shell 将 compositor 事件与该 ID 匹配，应用 manifest 显示模式并激活不透明表面令牌。令牌绝不会由 WASM 提供。

Home 会隐藏应用但不会终止它。Tasks 显示活跃任务卡片，并提供明确的 OPEN 和 STOP
操作；OPEN 是安全的默认操作，Space 是直接执行 STOP 的快捷键。Apps 对选中的 RUNNING
或 STARTING 应用提供同一快捷键，其 Actions 详情页则根据状态显示 OPEN APP 或 STOP APP。
STOP 命令发送给 appd，由 appd 清除应用的会话权限并终止其临时 systemd 单元。当
compositor 撤回 surface token 时，已安装应用的 Launcher 行仍会保留，并恢复为 READY。

## 防御客户端

Shell客户端验证协议版本、请求ID、响应类型、应用ID语法、显示模式、布尔值、分页进度和所有有界字符串副本。目录调用使用500 ms套接字超时；生命周期调用允许systemd 3秒。每个套接字在连接前接收`FD_CLOEXEC`。

纯C测试覆盖分页、元数据解码、响应身份、无效显示模式、错误请求ID、输出数组不足和恶意应用ID。UI测试覆盖安装/停止状态、启动、表面绑定、在目录删除时不移除令牌、启动失败状态、任务操作和滚动超出四条记录。应用和任务有320x170像素的回归快照。

## V0.6 验证

更新后的AArch64 appd、cp0ctl和System Shell无需重启或刷写镜像即可热部署。设备检查确认appd返回了`Hello Card`、`standard`以及可信清单中的启动/停止生命周期状态。外部启动生成了一个compositor表面令牌，Shell显示了标准的通知权限提示，一次性授权完成了WASM主机调用，停止返回了catalog并保留了安装条目。摄像头检查确认了提示和最终的Home屏幕。compositor、Shell和appd保持活跃。

设备故意没有远程输入注入接口。Apps -> Hello Card -> Enter 和 Tasks -> STOP 的最终物理验收仅剩一个简短的操作员检查；所有底层状态转换和实际的 appd/表面路径已经独立地进行了测试。
