# M1-A Protocol Compatibility Note

本文记录 M1-A 控制面协议相对 M0 的兼容边界，承接 `04-m1-experience-architecture.md` §4。

## 不变项

- `EventKind::ALL` 仍为 86；M0 历史 event payload、`stream_seq` 顺序和 replay 语义不变。
- M1-A query、trace、review 和 eval report 都是派生读取或既有治理事件，不建立第二条事实流。
- 旧的 Shell/File/MCP `ActionParameters` 编码形状不变。

## 新增项

- `forme-protocol::control` 增加 Surface/Gateway/cursor/control/review/manual-eval DTO，均带 `SchemaVersion`。
- `BackendKind::Notification` 与 `ActionParameters::Notification { surface, target, title, body_ref }` 是枚举新增 variant。旧 Rust exhaustive match 需要在升级时显式处理；旧 JSON reader 对未知 variant 应拒绝，不能静默映射成 Shell/File/MCP。
- Notification 在 M1-A 只占协议位，不注册 execution backend。planner/policy 继续 default-deny；实际 delivery 在 M1-B 经 immutable plan、approval 和 execution-time recheck 激活。

## 迁移要求

- M0 event replay 不需要数据迁移，因为 M1-A 没有写入 Notification action event，也没有改变既有 EventKind。
- consumer 升级时必须先更新 protocol snapshot tests 和 exhaustive match，再允许读取含 Notification 的新请求。
- `ApprovalDecision` 只映射为 one-shot grant；通用 `ResumeInput` 不属于 Web protocol。candidate confirm 复用 `CandidatePromoted { by=user }`，不新增事件别名。
