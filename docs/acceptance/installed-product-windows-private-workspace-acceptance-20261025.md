# Windows 私有工作副本与 Host 回写组件验收

日期：2026-10-25。分支：`shu/installed-product`。实现提交：`b0f5cfb`。

本报告承接 owner 已选择的 A 方案、PRD24 §4.1.2–3 和 canonical §29.9；它追加组件证据，不改写此前 AppContainer adapter 或 M0–M5 的历史结果。当前宿主为 Windows build 26200、非管理员进程；没有安装/启动 SCM 服务，没有读取真实 owner 配置、模型密钥或脑数据。

## 1. 实现范围

- 每 job 将已注册 workspace 的普通文件复制到独立目录。原 workspace 不授予 AppContainer，也不修改其 DACL、owner 或完整性标签；仅副本设置并回读 Low mandatory label。
- Host 私有 baseline 绑定原 job/plan/permit/spec、epoch/nonce/deadline、原根与副本 file identity 和完整清单。拒绝 reparse/symlink、硬链接、非普通文件和非规范名称；上限为 1024 条目、32 层、64 MiB 总数据、16 MiB/文件、4 MiB/记录。
- File 按精确路径和 Read/Write/Delete 核验，Write bytes 必须等于已批准内容；Shell 只接受显式 cwd 与 `network=false`，PTY 只接受显式 cwd，回写范围限该 cwd 子树。不支持的 backend/缺失 spec 在启动前拒绝。
- Host 只在 root 正常退出、Job 已空且原 permit 仍有效时回写；句柄 custody、file identity、完整 baseline 冲突检查与最终清单核验共同约束原目录副作用。owner/其他 job 的并发修改不会被覆盖。
- create-new、限长、flush 的回写 intent 先于原目录副作用；verified receipt 后才允许完成和清理。receipt 的 job/plan/intent/baseline/result 摘要在移除副本前保留到原 sandbox lease journal；拒绝迟到或冲突的证明。
- intent 后错误/缺失或损坏 receipt 保留原副本与证据，阻断新 admission；启动恢复只终止原树、撤 ACL 和回收原 profile，不重放、不自动回滚、不改写脑 Unknown。
- 非交互 worker 使用 `DETACHED_PROCESS`；目标 token 在 resume 前移除除可选 `SeChangeNotifyPrivilege` 外的全部特权并回读。OS child-process restriction 拒绝子进程，闭合环境只含固定 worker 字段及 OS 查询的 SystemRoot/LOCALAPPDATA，不继承 PATH、代理或秘密。

全部业务状态机和副本/事务模块继续禁止 unsafe。Win32 操作仅限已授权的两个 adapter 文件；没有新增 crate、内部依赖边或脑 EventKind。

## 2. 最新源码验证

| 门 | 结果与证据 |
|---|---|
| execution strict Clippy 与规模门 | PASS，`target/overlay-focused-clippy.log`；707 文件、62 处既有规模债务不增长。 |
| lease 证明顺序回归 | 8/8 PASS，`target/overlay-receipt-ordering.log`；晚于目录移除 intent 的新证明拒绝且 journal 不增长。 |
| 真实 AppContainer 合同 | 3/3 PASS，`target/overlay-native-final-source.log`；显式执行本次完整 workspace gate 编译的 integration binary 的 `--ignored` 场景。 |
| 统一源码质量门 | PASS，`target/overlay-final-quality.log`；171 Rust suites、1018 passed / 0 failed / 9 默认 ignored，前端 77/77，strict Clippy、NTFS、打包、签名回归与完整合规通过。3 个原生 worker 场景另外显式执行，不把默认 ignored 计为通过。 |
| installed-host 永久组件门 | 最终源码 161 个命名场景 PASS，`target/overlay-installed-host-final.log`。IP01–IP24 仍为 NOT_RUN。 |
| M5 永久总门及 current-tree release receipt | 尚待本次干净实现提交后重验；不沿用旧回执作当前 PASS。 |

真实 AppContainer 三个场景分别证明：

1. Low worker 可以读写临时私有副本，Host 核验后改变原文件；原 workspace 和兄弟目录访问拒绝，job/permit/spec 只读，Host baseline/journal 不可读写，注册程序及副本内程序均不能派生执行。宿主对同一 loopback listener 和公共 TCP 目标的连接成功，worker 连接均失败；环境闭合，清理后 reopen 成功。
2. worker 真实进入主函数并只写副本后，失败退出、取消、owner 并发更新或 adapter drop 不回写；owner 更新保留，原 lease 完成清理。
3. 强制结束实际 Host fixture 进程，跳过析构，再按原 journal/profile/Job 恢复；worker 不重放、原 workspace 不变，原租约精确清理。恢复失败时保留 fixture 路径和现场。

原始 overlay 单元/故障场景还覆盖精确 File 内容、cwd 外修改、超限、硬链接、祖先改名/并发写拒绝、partial intent 不重试和坏 receipt 不销毁证据。这些原生文件/进程证据使用隔离临时数据，不是实际 Brain/Web/keys 服务 ACL 的验收。

## 3. 限制与后续承重门

- 生产默认仍是 `UnavailableWorkerIsolation`，没有把本组件自动接到正式 dispatch。child-process restriction 意味着需要派生 shell/PTY 子进程的 profile 当前不可用，不能将 fixture 的成功解释成任意 Shell 能力已可用。
- 真实 LocalSystem Host 与已安装 core 的 OS peer identity、服务账户有效拒绝、会话 helper、管理员安装/冷启动、签名包和 LAN 客户端信任仍未通过。
- Linux/macOS 原生实现与原生平台证据仍开放。3 个 Windows worker 场景不提升 IP10/IP24，也不证明整个安装/常驻/Web 产品已完成。
- 构建继续复用同一个 target；缓存超过 64 GiB 预算时如实记录，不自动清理 target、运行数据、研究语料或验收证据。
