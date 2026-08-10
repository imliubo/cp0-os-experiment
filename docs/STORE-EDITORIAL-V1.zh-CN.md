# Store 编辑收藏 v1

<!-- doc-locale: zh-CN -->
> [English](STORE-EDITORIAL-V1.md) | **简体中文**

S8A 为 320x170 今日页面添加了一个经过审计的控制面路径，并将结果携带到使用签名 Catalog v4 的设备中。操作员只能从经过批准且当前发布的版本中选择内容。设备从不直接信任操作员 API 的响应。

## 边界布局

有一个v1布局，其稳定的标识为`today`。完整的替换包含：

- 一个1-48字符的标题；
- 一个特色版本；
- 一个或两个具有不同1至32个字符标题的集合；
- 每个集合中有 一到四个 Releases。

空白、首尾空白、控制字符、重复标题、重复 Release ID、重复 App ID 均被拒绝。特色应用不能同时出现在一个集合中。每个标识符必须解析为状态为 `published` 的 Release，其提交状态仍为 `approved`，且其 App ID 和版本仍与该提交匹配。

响应将每个 Release ID 解析为其权威的 App ID。操作员不会提交 App ID，因此他们无法创建 Release/App 不匹配。

## 操作员API

标准模式是 `schemas/store-control-v1.openapi.json`:

```text
GET  /v1/editorial/releases
GET  /v1/editorial/today
POST /v1/editorial/today
PUT  /v1/editorial/today
```

发布发现是一个包含1-50项的键集分页视图，覆盖不可变的Store制品。它仅包括经过批准的提交，其发布仍处于`published`状态，其制品匹配发布目录序列和身份，并且其序列是该应用当前发布的最新发布投影。服务器在返回其权威名称、版本和可选类别之前验证存储的目录应用。暂停、删除、被取代、损坏或无制品的行不能成为操作员候选人。相同的操作员、双因素认证、令牌和`store.editorial`检查保护了这一读取操作。

GET 返回当前布局和ETag。POST 创建资源版本1，需要`Idempotency-Key`，并明确拒绝`If-Match`；如果布局已存在，则返回409。PUT 用完整布局替换，需要`Idempotency-Key`和当前的`If-Match`，并增加资源版本。过时的ETag返回412，创建前的PUT返回404。

该API使用隔离的Store操作员身份和令牌域。访问需要有效的`editor`或`admin`，启用双因素认证，一个有效的非撤销令牌，以及精确的`store.editorial`范围。开发人员、审核员和操作员令牌不能共享摘要或跨身份域。

每次成功的写操作是一个可序列化的事务，包含：

- 当前的`store_editorial_layouts`行；
- 一个只读的`store_editorial_revisions`行用于记录确切的新版本；
- 一个 `editorial.today-created` 或 `editorial.today-updated` 审计事件；
- 一个 `catalog.rebuild-requested` 出箱事件绑定到主打的 Release 和新的 `editorial_resource_version`；
- 完整的幂等响应。

延迟数据库约束要求在提交前，布局、修订、审计事件和出箱事件必须一致。数据库触发器拒绝删除操作、非单调版本、不可发布的引用或缺少任何这些记录的更新。幂等重放返回原始主体和ETag，而不创建另一个修订或重建请求。

## Catalog v4 发布

编辑重建任务会从出箱事件携带`editorial_resource_version`通过发布任务和目录快照。发布者会重新加载那个修订版而不是当前可变行。这使得即使操作员创建了v2，延迟或重试的v1任务也是可重复的。

Catalog v4 扩展了带有签名的 Catalog：

```json
{
  "editorial": {
    "headline": "Reviewed for 320 x 170",
    "featured_app_id": "dev.cardputerzero.featured",
    "collections": [
      {
        "title": "Small-screen essentials",
        "app_ids": ["dev.cardputerzero.notes"]
      }
    ]
  }
}
```

投影仅在检查每个引用的发布版本仍具有匹配其权威App ID和版本的发布 artifact 后使用 App ID，并且每个 App 都存在于相同的 Catalog 应用集中。Catalog v4 需要 v2 发现字段、v3 资源字段和有效的编辑元数据；较旧的模式版本必须不包含 `editorial` 字段。

如果引用的 Release 被暂停、移除、被更新取代或不在预期应用集中，正常构建会按 fail-closed 原则生成不含 editorial data 的 Catalog v3。恢复该 Release 后，后续重建可回到 v4。绑定了 editorial revision 却缺少对应 immutable revision 的任务属于硬发布错误，不进行回退。

驱动发布重建任务不绑定单独的编辑修订。它们在准备快照时读取当前有效的布局；签名的目录字节仍然绑定精确发出的投影，而快照的`editorial_resource_version`列仍然为空。该列记录显式的编辑任务来源，而不是目录是否包含编辑数据。

## 设备IPC和UI

`cp0-stored` 验证目录签名、方案、集合边界、唯一的 App ID 以及在签名应用程序集中的成员资格。本地协议增加了一个严格的请求：

```json
{"protocol_version":1,"request_id":7,"command":{"name":"today"}}
```

响应重复 `sequence`, `expires_unix_seconds`, 和 `stale`，然后
返回 Catalog v1-v3 的 `editorial: null`，或者返回一个有界的 Today 对象。
特色和集合项是来自同一经过验证的 Catalog 的完整 `StoreAppSummary` 值，包括当前安装/更新操作状态。
没有 URL, Release ID, 数据库字段或未签名的操作文本跨越本地
IPC 边界。

System Shell 在获取 Catalog 后立即获取 Today，并且只在两个响应的 sequence 完全相同时接受。空布局、解析错误、IPC 错误或 sequence 不匹配会清除所有 editorial 状态。后台 Catalog 刷新会在所选 collection 和应用仍然存在时保留其稳定标题与 App ID。

在320x170单前景UI上，Today展示一个特色应用和最多两行收藏。Enter键进入特色应用详情或选择的收藏；一个收藏最多展示四个应用。收藏内禁止左/右切换。Escape键先关闭详情，再关闭收藏，最后离开Store导航。

## 验证

PostgreSQL 接受测试涵盖有界当前 Release 发现和游标验证，操作员身份验证，创建/重放/更新，过时的 ETag，无效或重复引用，不可变版本，直接 SQL 篡改，审计/出箱回滚，精确发布者版本重放，Release 暂停/恢复/移除回退，任务超载，以及快照来源。
协议和守护进程测试涵盖目录 v1-v4 兼容性和严格 Today 投影。系统 Shell 测试涵盖解析，导航，刷新连续性，操作传播，以及精确 320x170 的像素快照。
