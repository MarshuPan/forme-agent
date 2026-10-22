# 产品化实施中的可维护性检查

## 1. 范围

本记录对应 owner 要求的工程规范与大模块维护，不代表安装器、五面 Web 或全部历史技术债已经完成。规范入口为 [code-quality.md](../engineering/code-quality.md)，产品范围仍按 prd/24–26。

| 提交 | 结果 |
|---|---|
| `3236e50` | 固定 Rust API/格式、错误、并发与恢复、Web/宿主、测试/审查规范，说明规模阈值是仓库维护约定。 |
| `b64fc04`、`b3b779f` | 自动检查冻结依赖方向、crate unsafe 禁令和文件规模；CI 运行同一检查器，标准 tests.rs 使用测试文件阈值。 |
| `b373aec` | 按控制域与 HTTP 职责拆 Gateway，保留 public API、事件、测试名字与生产装配。 |
| `57e34b8` | M4/M5 审计定位到拆分后的正式测试/实现；新增旧文件诱饵反向测试，不能用残留同名文字代替新文件内容。 |
| `26ce4b4` | `forme-hostd.rs` 从 783 行缩到 40 行；配置/凭据、HTTP、进程装配、SCM 信号分别为 217/294/188/70 行。移除该文件已偿还的规模豁免。 |

拆分没有新增 crate/依赖边、脑 EventKind、公开库 API、第二 supervisor 或另一套运行时。两域 listener、环境配置、请求格式、已有服务名与恢复语义保持不变。

## 2. 验证

最终 Rust 源码 `26ce4b4` 已执行 fmt、workspace check、完整 workspace tests、strict Clippy、合规和 `git diff --check`；结果 688 passed / 0 failed / 5 默认 ignored。宿主专项 60 个命名场景执行通过；含真实子进程/loopback、分域拒绝、并发幂等、取消、原 job Unknown 与受控停服，不是原生安装证据。

Python 工具全量运行 72 项，其中 1 项按平台跳过，其余通过。结构门扫描 341 个源文件；剩余 **65 处历史规模债务**，未扩大基线。M5 永久长链与当前回执结果在 [M5 报告 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验) 单独记录。

## 3. 后续维护约束

- 新生产文件原则 ≤600 行，测试 ≤1200 行。历史超限文件按审查基线不增长；缩至正常范围后移除旧豁免，不保留可回涨的额度。
- 后续优先处理 harness 的运行协调/production 和 store 的事务领域；每批先保持 API、事件顺序、事务与故障恢复，再做独立提交和正反验证。
- 历史债务是公开库存，不是已完成的可维护性认证。不得机械切片、用 include 拼接、增加转发空壳或扩大基线来让数字变好；最新库存见下节。
- Web 新功能按页面/client/session/reducer、服务端认证/配置/bridge 分责，不继续堆入现有大文件。工具门不能替代行为审查、原生权限和产品 IP 证据。

## 4. 受管服务与工作台增量（2026-10-21）

本批拆出 harness ingress、production builder、capability 文档解析和 models 文件凭据 resolver；`production_config.rs` 从历史 841 行降至 493 行，解析子模块 356 行，并移除已偿还的规模豁免。harness lib 约 7390 行、store sqlite 约 8975 行，仍按历史棘轮管理，不宣称全仓大模块债务已清。

结构门当前检查 414 个源文件，剩余 **63 处历史规模债务**，没有扩大基线。`harness/src/brain_orchestration.rs` 已完成首批职责拆分：节点构造、输出归并、签名转换和独立验证移到 `brain_orchestration/support.rs`；公开 API、事件顺序、事务边界和恢复语义保持不变，C16 编排全套 23 项回归通过，并移除该文件的旧规模豁免。统一源码门新增 TypeScript、前端行为、可重复资源和 bundle 保留/完整性验证；GitHub CI 使用同一脚本。组件与最终源码结果详见 [受管服务与工作台组件记录](installed-product-managed-workbench.md)。本节数值更新不覆盖上节 `26ce4b4` 的历史测试回执。

## 5. 模型配置增量（2026-10-22）

`01b77f9` 将配置事务按状态协调、journal、文件发布、模型校验和测试分责；前端将草稿转换与受控启停从设置页拆出。配置入口 389 行，journal/materialize/storage 分别为 304/348/103 行，启停组件 73 行；没有使用 include 拼接、扩大旧基线或新增内部依赖边。

最终结构门扫描 433 个源文件，仍为 **63 处历史规模债务**；新增文件均符合 600/1200 行约定。全量 Rust 749 passed / 0 failed / 5 默认 ignored，strict Clippy、14 项前端测试、7 项 bundle 回归与完整合规门通过。已认证浏览器检查了本次设置交互，其他页面与原生平台证据不由它替代。详细范围和可重复命令见 [配置闭合验收](installed-product-configuration-acceptance.md)。
