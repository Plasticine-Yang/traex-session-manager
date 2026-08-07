# tsm 构建 ticket(执行目录)

> 本目录是照 [`../tsm/spec.md`](../tsm/spec.md) 实现 tsm v1 的**执行 ticket**。
> `../tsm/`(map + issues 01–10 + research + prototypes)是 wayfinder 决策链的**已解决存档**,本目录不改动它。
> 每个 ticket 是 tracer-bullet 垂直切片,`Blocked by` 声明其阻塞边;发布顺序 = 依赖顺序(blockers 优先)。

## 依赖形状

```
01 ──┬─→ 02 ─→ 03 ─→ 05 ─→ 06 ─→ 07
     │                    ↑
     │        02 ─────────┘
     ├─→ 04           (06 also blocked by 02)
     └─→ 08
```

- `01` 走通骨架,是所有 ticket 的根。
- 01 后开三条战线:**02**(主链起点)、**04**(改名,并行)、**08**(分发,并行)。
- 主链:`02 → 03 → 05 → 06 → 07`;`06` 额外依赖 `02`(要 lifecycle 视图)。

## 工作方式

工作 **frontier**:所有 blocker 均已 `done` 的 ticket。每个 ticket 用 `/implement` 建,ticket 间 `/clear` 上下文(每个 ticket 自包含)。`/implement` 内部驱动 `/tdd`,收尾跑 `/code-review` 再提交。

| # | 标题 | Blocked by |
|---|---|---|
| 01 | 走通骨架:启动 → 当前项目活跃列表 → 退出 | 无 |
| 02 | Scope + Lifecycle 切换 + 预览面板 | 01 |
| 03 | 搜索模式 + 多选 | 02 |
| 04 | 改名(第一条写路径) | 01 |
| 05 | 批处理引擎 + 删除(单删 + 批删) | 03 |
| 06 | 归档 / 取消归档(复用批处理引擎) | 05, 02 |
| 07 | 运行时健壮性 + 刷新 + 帮助 | 06 |
| 08 | 分发与发布 | 01 |
