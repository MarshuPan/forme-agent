# 受管 MCP 发现协议基础层验证（2026-11-10）

状态：`shu/product-integration` 分支上的协议与治理组件通过，尚不是安装版 MCP 可用、三项接入方案完成、完整 D/Final 或 IP 验收通过。owner 对发现动作的批准已在 `2acc6cf` 激活，本范围无需重复确认。

## 实现范围

- `2b4124c` 独立拆分动作参数和策略目标匹配模块，保留 public reexport、旧字段顺序、serde 行为与动作摘要；原有超限文件缩小，规模基线未刷新。
- `a31ffbf` 末尾追加 `ActionParameters::McpDiscovery`，落实 closed/versioned spec、工具/资源 catalog、配置与 catalog 摘要、引用/文本/JSON/完整结果预算校验。旧 MCP、WorkerJob、WorkerPermit、WorkerExecutionSpec 的固定字节摘要回归保持。
- 规范化资源为 `mcp-discovery:{server}` 和 `program-registration:{program_ref}`；完整 spec 参与原 action/plan digest。当前配置的 scope、allowlist、enabled、trust、timeout、output_budget 等任何变化均不能沿用旧 settings 绑定。
- 发现动作固定为 `Mcp / Execute / High / Outward / OwnerControl`，使用独立 capability `mcp-discovery:{server}`。策略要求请求专属 Session 层 `McpDiscoveryTarget`，绑定完整 action digest 且 effect 为 Ask；普通 MCP allowlist、旧工具 matcher、Any 或 Allow 规则均不能代替它。有效 Deny 不会被委托 envelope 的 Ask 提前返回掩盖；Agent/Subagent 来源或委托不能转成 owner discovery。
- Planner 拒绝截断式预算与语义不一致的发现动作；worker spec 再检查原作业剩余期限和原执行预算。普通进程内 McpBackend、Remote operation 和模型 formal-tool 参数入口明确拒绝此分支。
- catalog 摘要不提升信任；返回 `TrustTier::Untrusted`，对应后续 context 的 UntrustedData 处理。元数据尚未接入 registry 发布路径。

## 已取得的组件证据

兼容拆分：194 个测试目标，1,458 passed、0 failed、22 ignored；fmt、整仓 check/test、strict Clippy、质量和完整合规全部通过。原回执为 `target/product-integration-evidence/discovery-action-extraction-gates.json`，1,024 项源码及六份日志已逐一独立复核。

实现：197 个测试目标，1,474 passed、0 failed、22 ignored；fmt、整仓 check/test、strict Clippy、产品二进制构建、质量和完整合规七项全部通过。回执为 `target/product-integration-evidence/discovery-protocol-final-gates.json`，SHA-256 为 `a85259f580df31742ae7efb5c2c844dd854ff0e66215dd953f39d6e515e23b56`；1,030 项源码及七份日志已逐一独立复核。

新增覆盖包括：未知字段/版本与边界值、每项配置漂移、恶意/超长 schema 与文本、重复目录项、完整原始正文超预算、原 plan/permit 逐字段替换、独立 owner 请求/拒绝/审批、缺 sandbox/权限、模型入口与 Remote operation 拒绝、进程内 backend 零执行事件、原 job 剩余期限，以及旧 wire/digest 固定值。整仓忽略项没有被改为通过。

构建继续使用本工作树固定 target、原 dev/test 空间参数和两个编译并发。实现检查结束时分配空间为 59,389,075,958 bytes（约 55.3 GiB），低于 64 GiB；原报告为 `target/product-integration-evidence/discovery-protocol-final-build-resources.json`。未清空构建缓存，也未重建或覆盖已有签名安装候选。

## D 波运行场景

干净提交 `482b5d3e86662a33f4ce8c064ab654063b3503cf` 上实际执行 `tools/verify-core-brain-closure.ps1 -Wave D -RuntimeOnly`，73/73 通过，exit 0；随后通过 `tools/core_brain_evidence.py` 的独立完整校验。原日志 `discovery-protocol-clean-d.log` SHA-256 为 `8fcf6e8b350ad5425b4d63c8d46a7a77dfd478423c0780a92414db21301c6b6f`，`discovery-protocol-clean-d/runtime.json` SHA-256 为 `c53e7694336bcb5022a6b3ed27f97436a3caf07c9730f2c7a1c7dcc38053ca5c`，均位于上述本地证据目录。

这是该源码的 D RuntimeOnly 证据，未执行 C24 live evidence，不表示完整 D、Final 或原生安装/IP 通过。

## 发布审计阻塞及修复

`a31ffbf` 的完整 M5 调用在 S99 当前供应链审计停止，exit 1，尚未到最终仓库回执比较。此前命名场景、M4/M5 golden、完整 workspace/Python 与合规均通过；失败日志 `target/product-integration-evidence/discovery-protocol-clean-m5.log` 的 SHA-256 为 `79a9c032e4720c16f446930d9bfbc97abc530dfd12da3d82232c45dc6f7f1848`。

真实阻塞是 `png 0.18.1`、`crc32fast 1.5.0` 漏写 NOTICE，以及 Windows 发布目标可达的 libsqlite3-sys 0.38.1 命中 RUSTSEC-2026-0459。`53322c2` 先记录修复范围及依赖声明，`482b5d3` 将 rusqlite 精确升级至 0.41.1、锁定 libsqlite3-sys 0.39.1 / SQLite 3.54.1。四处 backup 调用改用新版公开的 NO_PROGRESS 常量，保持无回调语义；不改 store schema、事务、事件或恢复权限。首次编译的类型推导失败记录保留于 sqlite-remediation-final-*。

修复后 197 targets、1,474 passed / 0 failed / 22 ignored；fmt、workspace/all-targets check/test、strict Clippy、产品二进制构建、质量与完整合规七项 PASS。`sqlite-remediation-verified-gates.json` 的 SHA-256 为 `3f66f21d1be1ba84f058cbe4bde95c608f4ab07b613166da36ba1f7039f64aed`，1,030 项源码与七份日志逐项独立复核通过。新的 cargo-audit 扫描不再含 SQLite 公告；非发布目标依赖的 quick-xml 等告警继续保留，不能把原始全 lockfile scanner 的 exit 1 宣称为全平台零漏洞。当前树的完整发布门和规范回执结果另见 [M5 验收记录](m5-acceptance-report.md)。

## 本轮构建空间维护

新依赖重建后，当前工作树缓存分配量为 82,792,821,755 bytes，超过原 64 GiB 报告阈值。确认没有 Cargo/rustc 和当前缓存程序占用后，仅对当前 target/debug 内 94 个大型 exe 执行可逆 NTFS 压缩；每份逻辑长度和内容摘要保持，Git HEAD/dirty 状态保持，未删除缓存、源码、安装包或证据。共释放 21,645,462,911 bytes（约 20.16 GiB）。

完成后的正式空间报告为 61,147,358,844 bytes（约 56.95 GiB），低于 64 GiB；逻辑大小保持 85,956,261,464 bytes。维护回执 `sqlite-cache-compression.json` SHA-256 为 `346357d6b1a59100827fde87ba00d5931500a57e808aba1b5feb8f56e48dd342`；空间报告 `sqlite-remediation-after-compression-resources.json` SHA-256 为 `69e2a1868c2c61dc31644a93efab8267ca49d1a2c0c74fea0aa63f48f95959b1`，二者均在上述本地证据目录。后续专项门可能产生新的缓存，其结束报告独立记录，不将阶段采样当作瞬时峰值。

## 尚未完成的执行链

本批没有启用 MCP 的产品配置或发现入口。必须继续实现 Host 受保护程序注册/撤销、正式 AppContainer discovery/call worker、有限 RPC 分页和共用 deadline/输出预算、结果原件封存与 Core 当前配置重查、受保护 metadata 登记/删除/重启恢复，以及已认证 owner 的显式 Web 入口。不能仅因类型、摘要和策略测试通过就把消费者标为可用。

原 job 的 admission window 必须容纳已批准的发现 timeout；排队后不足时按当前校验拒绝，不修改原 spec、不延长原 permit、也不换号重做。当前普通 MCP 工具调用路径和原 Unknown 恢复规则保持。

本轮没有运行 Windows 原生升级、桌面截图或浏览器产物验收。之前的 UAC 启动未确认记录继续保留，不能归因为用户取消，也不能把本轮源码/构建回执替代原生结果。插件、渠道与完整 IP01–IP24 的剩余义务继续有效。
