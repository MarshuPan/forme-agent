# prd/17 原创 + 合规 CI/doctor(Originality & Compliance)

模块实施级 PRD。**构建期工具**(非运行时 crate)——原创性与合规边界从第一天可执行。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D15**(原创与合规:CI/doctor 边界)。
- **架构**:无运行时 crate(build-time tooling);贯穿全工程。
- **口径**:canonical **§12**(开源原创 CI 门)。
- **验收**:主证 **S22**(原创 CI/Doctor)。
- **不偏离**:M0 即建 CI 门(不等发布前);完整法律审计留发布期。

## 1. 职责与边界

- **做**:CI/doctor 原创合规检查——`research/upstreams/**` 不进 build/import/include;不复制第三方源码/文档/prompt/错误信息/目录结构/命名/测试 fixture;GPL/未知许可证默认不进主工程;来源记录(source/license/copy_risk/implementation_rule/notice)。
- **不做**:运行时逻辑;完整法律审计(发布期);LICENSE/NOTICE 定稿(发布期)。
- **crate**:CI/doctor tooling(非运行时)。**M0 范围**:CI 门 + doctor;完整法律/依赖安全审计 = 发布期。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:无(独立工具)。
- **下游被依赖**:build/release pipeline(检查失败 → 阻断 release-ready)。
- **发出事件**:`ComplianceCheckResult`(N 组)。
- **必守不变量**:`research/upstreams/**` 不进 build/import/include;不复制第三方表达;GPL/未知许可证默认不进主工程;外部来源须记录;**检查失败阻断 release-ready**。

## 3. 数据结构与类型

```rust
pub struct ComplianceCheck { pub scope: CheckScope, pub outcome: CheckOutcome, pub blocking: bool }
pub enum CheckScope { UpstreamIsolation, License, CopyDetection }
pub enum CheckOutcome { Pass, Fail(Vec<Finding>) }
pub struct BorrowingRecord {   // 需公开协议兼容/依赖/复用时
    pub source: String, pub license: License, pub copy_risk: CopyRisk,
    pub implementation_rule: String, pub notice_action: NoticeAction,
}
```

## 4. 接口与签名

```rust
pub trait ComplianceDoctor {
    fn check_upstream_isolation(&self) -> ComplianceCheck;   // research/upstreams/** 不进 build/import
    fn check_license(&self) -> ComplianceCheck;              // GPL/未知默认不进主工程
    fn check_copy(&self) -> ComplianceCheck;                 // prompt/错误/目录/fixture 复制检测
    fn record_borrowing(&self, r: BorrowingRecord) -> Result<()>;
}
```

## 5. 行为/算法/状态机

- **检查**:(a) 主工程 import/include/build 引用 upstream → 失败;(b) GPL/未知许可证材料进主工程 → 失败;(c) 复制第三方 prompt/错误信息/目录/fixture → 标记。→ `ComplianceCheckResult{outcome=fail, blocking=true}`。
- **合规树**(upstream 仅在 research/ 且不被引用)→ 通过。
- **语料完整性**:linked worktree 必须从 Git common dir 定位主 worktree 的本地 `research/upstreams/`；语料缺失、不可读或 scanner 失败均 fail closed，不能把“没有扫描输入”解释成 PASS。
- **误报边界**:标准语言/框架样板（例如仅含常见 trait 的 Rust `derive`）不单独作为长 prompt/错误/文档表达命中；exact/normalized/token/naming/directory 检查仍继续执行。
- **来源记录**:需协议兼容/依赖/复用时记 `BorrowingRecord`(source/license/copy_risk/implementation_rule/notice)后放行。
- **失败 → 不 release-ready**。

## 6. 错误/安全/权限/审批

- **检查失败阻断 release-ready**(硬门)。
- 来源不明或未获授权的材料不进 release tree。

## 7. 配置与可观测

- **config key**:`compliance.upstream_paths`、`compliance.license_allowlist`、`compliance.copy_patterns`。
- **可观测**:`ComplianceCheckResult` + findings 列表(CI 红/绿)；release audit 的 copy check 输入摘要同时绑定 release tree digest 与 upstream corpus digest。clean release tree 的 tracked 输入按 Git index mode + blob identity 摘要，checkout 的 CRLF/LF filter 不得改变 receipt；working tree dirty 仍 blocking。

## 8. 实施步骤与顺序

1. `UpstreamIsolation` 检查(research/upstreams/** 不进 build/import/include)。
2. `License` 检查(GPL/未知默认不进主工程)。
3. `CopyDetection`(prompt/错误/目录/fixture 复制检测)。
4. worktree corpus 解析 + corpus digest；缺失或不可读时阻断。
5. `BorrowingRecord`。
6. CI 门(失败阻断 release-ready)。

## 9. 测试

- **S22(总纲 §7.5)**:构造 upstream import / GPL 材料 / 复制内容 / corpus 缺失 → blocking fail;合规对照树与 linked worktree 共享 corpus → 通过;来源记录后放行。
- **单元**:三项检查各自生效;失败阻断 release-ready。

## 10. 验收

- research/upstreams 不进 build;linked worktree 不漏扫且 corpus 缺失 fail closed;不复制第三方表达;GPL/未知默认不进;来源记录;失败阻断 release;S22 通过。

## 11. 原创性与合规

- 本模块**即**原创性机制——自指:它 enforce canonical §12。检查逻辑为本项目自有,不依赖任何第三方合规工具的代码/规则表述。
