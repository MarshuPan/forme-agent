# prd/16 模型 + 配置 + ConfigDoctor(Model & Config)

模块实施级 PRD。归属 `models` + `config` crate。模型渠道接入(经 prd/05)、配置分层与密钥分离、配置体检。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D13**(ModelProvider/ModelProfile/credential/base_url/capability/cost·rate limit/config precedence/secrets/ConfigDoctor)。
- **架构**:architecture/03 §1(models/config crate);ModelProvider 经 prd/05 接入框架;设计底稿 archive/planning/08(ModelProfile/ToolCompatibility)。
- **口径**:canonical **§20**(模型能力自适应:协调按 ModelProfile 调"上多少结构")、**§9**(config)。
- **验收**:主证 ConfigDoctor 矩阵行。
- **不偏离**:model call 编排在 loop(prd/03);接入生命周期在 prd/05;本篇是模型 provider + 配置 + 体检。

## 1. 职责与边界

- **做**:`ModelProvider`(经 prd/05)、`ModelProfile`、`ModelRoutingPolicy`、`ToolCompatibility`、config(typed/precedence/secrets 分离)、`ConfigDoctor`。
- **不做**:model call 循环(prd/03 loop 调 `ModelProvider`)、接入 lifecycle(prd/05)。
- **crate**:`models`、`config`(依赖 protocol、prd/05)。**M0 范围**:provider 抽象 + 配置 + 体检;复杂路由优化 = M1。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02、prd/05(接入框架——`ModelProvider` impl `ExternalProvider`)。
- **下游被依赖**:prd/03(loop 调 `ModelProvider`;`SessionBound` 绑 `ModelProfile`;resolve 时用 `ToolCompatibility`)、prd/12(coordination 读 `ModelProfile` 做能力自适应脚手架,canonical §20)。
- **发出事件**:C 组 `ModelCall*`(由 loop 发,本篇提供 provider);`ConfigDoctorReport`(N 组)。
- **实现 trait**:`ModelProvider`(impl prd/05 `ExternalProvider`)。
- **必守不变量**:**model call 经 loop/harness**(不直连)、**credential secrets 分离**、**能力自适应**(coordination 读 ModelProfile,canonical §20)、**config precedence**。

## 3. 数据结构与类型

```rust
pub trait ModelProvider {                             // impl prd/05 ExternalProvider
    fn call(&self, req: ModelRequest) -> Result<ModelResponse>;   // 由 loop 调
    fn profile(&self) -> ModelProfile;
}
pub struct ModelProfile { pub provider: ProviderId, pub base_url: Url, pub capability: ModelCapability,
    pub cost: Cost, pub rate_limit: RateLimit, pub credential_ref: SecretRef }
pub struct ModelCapability { pub context_window: u32, pub tool_use: bool, pub strength: ModelStrength }  // 弱→加结构、强→减结构(§20)
pub struct ModelRoutingPolicy { /* M0 简单 */ }
pub struct ToolCompatibility { /* 模型↔工具 schema 兼容 */ }

pub struct Config { pub layers: ConfigLayers /* precedence */, pub secrets: SecretStore /* 分离 */ }
pub struct ConfigDoctorReport { pub checks: Vec<DoctorCheck>, pub findings: Vec<Finding> }
```

## 4. 接口与签名

```rust
// ModelProvider 见 §3
pub trait ConfigDoctor {
    fn check(&self) -> ConfigDoctorReport;   // provider/credential/capability/MCP/plugin/fs·shell/scheduler/notification
}
```

## 5. 行为/算法/状态机

- **ModelProvider**:provider 抽象(经 prd/05 registry);`ModelProfile` 在 `SessionBound` 绑定;用量进 `ModelCall*` 事件(loop 发)。
- **config precedence + secrets 分离**:分层合并;credential 经 `SecretRef` 引用、不落明文。
- **ConfigDoctor**:检查 provider/credential/capability/MCP server/plugin manifest/filesystem·shell permission；M1-B 追加 scheduler tick/lease/max claims 与 notification availability/permission → 报告 + 解释；**配置错误显式报告,不表现为运行时猜测**。
- **能力自适应(canonical §20)**:coordination 读 `ModelProfile.capability` 决定脚手架——弱模型加结构(更细分步/更多验证)、强模型减结构;不可靠 fallback → 更保守(能力门封更低)。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:配置错误经 ConfigDoctor 显式化(非运行时猜测)。
- **credential secrets 分离**:不入事件/日志明文。
- **ConfigDoctor** 捕获错配。

## 7. 配置与可观测

- **config key**:`models.<id>.{base_url,capability,cost,rate_limit,credential_ref}`、`config.precedence`、`secrets.store`；M1-B 追加 `scheduler.{enabled,tick_ms,lease_ms,max_claims}` 与 `notification.{enabled,availability,permission}`。
- **可观测**:`ConfigDoctorReport`;C 组事件(loop)。

## 8. 实施步骤与顺序

1. `ModelProvider`(impl prd/05)+ `ModelProfile`。
2. config(typed/precedence/secrets 分离)。
3. `ConfigDoctor`(各项检查 + 解释)。
4. `ToolCompatibility` + 能力自适应输入(ModelProfile 供 coordination)。

## 9. 测试

- **ConfigDoctor 矩阵行**:provider/credential/capability/MCP/plugin/fs·shell/scheduler/notification 可检查 + 解释;配置错误显式非运行时猜测。
- **单元**:config precedence;secrets 分离(无明文泄漏);ConfigDoctor 捕获错配;弱/强模型脚手架输入不同。

## 10. 验收

- ModelProvider 经 prd/05;ModelProfile/routing/capability;config precedence + secrets 分离;ConfigDoctor;能力自适应输入;ConfigDoctor 行通过。

## 11. 原创性与合规

- model provider / 配置分层 / doctor 是通用模式。
- **不复制**第三方 provider/config 源码、命名或 prompt;引入第三方 SDK 时记 source/license/copy_risk/implementation_rule/notice。
