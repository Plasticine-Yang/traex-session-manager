# T-rename — 会话改名交互与写库落地

Type: grilling
Status: resolved
Blocked by: 03

## Question

你明确要的:在 tsm 里直接改会话名,免去"进会话 → /rename"的弱智流程。定清:

1. **触发与编辑交互**:选中一行按某键(如 `r`)进入 inline 编辑;文本框预填当前 title;Enter 提交、Esc 取消。是 inline 就地编辑还是弹一个输入框?
2. **写入落地**:依据 R3 结论 —— 只写 `threads.title`,还是需同步 `session_index.jsonl` 的 `thread_name` / 其他位置?是否同时更新 `updated_at`。
3. **并发/占用**:该会话正被 resume 打开时改名的行为(依 R3)。写库若 `SQLITE_BUSY` 如何重试/提示。
4. **校验**:空标题?超长?特殊字符/换行是否允许(title 会显示在 traex 选择器里)。
5. **失败与刷新**:写库失败如何提示;成功后如何刷新该行显示。
6. **批量改名**:是否支持(推测否,改名是单条语义)—— 确认排除。

## 交付

`## Answer` 记录:编辑交互、写入的确切 store 与字段、校验规则、失败处理、刷新方式。用 `/grilling` 推进,强依赖 R3 的 go/no-go 与安全参数。

## Answer

用户 2026-08-08 grilling 六项全数采纳。改名交互与写库落地定稿如下(写库安全参数全部继承 R3 的 GO 结论,此处不复述,只定 tsm 自己的交互与规则)。

**1. 触发与编辑交互（承 T-layout `Rename{buf}` 范式）**
- `r` 在光标行**原地弹出单行行内输入框**（非弹窗）；底部提示 `[Enter] save [Esc] cancel`。
- 预填 = 该行**原始 `threads.title` 原文**，光标置于末尾；标准单行编辑（←/→、Home/End、Backspace/Delete）。
- 边缘情形：若 `title` 为空（列表靠 `first_user_message` 兜底显示），输入框预填**空**——所见即 store 真实 title，不预填兜底文案。正常会话 `title` == 自动标题（R3 实证），故绝大多数情况预填即列表所见。
- `Enter` 提交、`Esc` 取消（丢弃 buffer，还原原 title 显示）。

**2. 写入落地（承 R3 GO）**
- 只写 `threads.title`：`UPDATE threads SET title = ?1 WHERE id = ?2`（与 traex `/rename` 逐字节一致）。
- **不 bump** `updated_at`/`updated_at_ms`——与 traex `/rename` 一致，排序位置不变（改名是元数据非活动）。R3 硬约束：若将来要"改名置顶"只 bump 秒级 `updated_at` 让 trigger 同步毫秒，**绝不单写 `updated_at_ms`**（秒/毫秒 skew）。
- **v1 不同步 `session_index.jsonl` sidecar**：选择器只读 `threads.title`、resume 不回读 sidecar，同步对正确性非必需；保持 tsm"唯一写操作只写 title"的干净地基。将来若有 traex 特性开始信任 sidecar 再补（按其 schema、last-write-wins）。
- 连接参数继承 R3：`SQLITE_OPEN_READ_WRITE`（**不带 `_CREATE`**）+ 写前 `conn.busy_timeout(5000)`；不改 journal_mode。

**3. 校验规则（提交时按序清洗/拦截）**
1. 首尾空白：**trim**。
2. 内嵌换行/Tab：**折叠成单空格**（宽容；title 会显示在 traex 选择器，不能带换行）。
3. trim 后为空：**拒绝提交**，保留旧 title，提示"标题不能为空"，**停在编辑态**让用户改（不写 DB）。
4. 长度：**不设硬上限**（DB 不限、traex 自动标题本就长；靠列表 `unicode-width` 截断显示）。
- 说明：以上为 tsm 自定校验，未严格逐字节对齐 traex `/rename` 的内部校验（未去 research）；DB 层 `title` 为 `NOT NULL`，空标题在 tsm 侧已被规则 3 拦下，不会走到写 `''`/`NULL`。

**4. 并发/占用（承 R3 残留风险）**
- 同会话正被 traex 打开时的 last-writer-wins race：**v1 接受，不做检测/加锁**（无法可靠检测"正在打开"的会话，且需 traex 在写后自身 `/rename`/auto-name 才会覆盖，概率低）。正常 resume/turn 不覆盖 tsm 写入（R3 实证）。

**5. 失败与刷新（R3 给事实，此处定 UX，均非致命不崩）**
- **成功**：重查库刷新 `all_rows`（R1 短只读事务）；光标按 session id 停回原行（不用索引位，见 T-layout `HashSet<SessionId>` 同理）；行尾短暂"已重命名"提示。
- **rowcount==0**（id 已被别处删）：提示"会话已不存在，可能已在别处删除"，刷新列表、丢弃该行。
- **超时后仍 `SQLITE_BUSY`**（罕见）：提示"库忙，请重试"，**保留已输入 buffer 停在编辑态**，再按 `Enter` 重试，无需重打。
- **rowcount 异常 >1**：不可能（`id` 主键），无需处理。

**6. 批量改名：排除**
- 改名语义天然单条（一 title 对一 id）。即便列表有多选（多选服务于删除/归档），`r` **恒定只作用于光标行**、忽略多选集。v1 不提供批量改名。

**对 CONTEXT.md / ADR 的影响**：无。`Session`/`title`/`Store` 术语已定义；本 ticket 全是交互与写库实现规则，`CONTEXT.md` 作为纯词汇表不收录。"改名=唯一写库"的硬决策早在 charting + R3 已定，T-rename 仅为其可逆的交互落地，不新增 ADR。
