# M5 受治理能力生态范围决策

本文承接 `02-capability-requirements.md`、`11-m4-scope-decisions.md`、M4 最终验收报告和 `architecture/canonical-contract.md` §1-26，定义 forme M5 的冻结范围。M5 把已经存在的 provider/capability/plugin/skill/MCP/connector/agent-profile 接入缝扩展为一个可验证、可撤销、可审计的能力包生态，但不把外部 catalog、publisher 或 package 变成新的 authority。

M5 的核心命题是：**forme 可以从真实生态发现、验证、安装、升级、撤销并向受认证 peer 分发能力，同时保持安装不等于信任、签名不等于权限、package 不等于 Agent 大脑。**

本文按 owner 指令冻结为 M5 实施输入；实现只能落实本文，不得在模块内另造 package/publisher/admission/lifecycle/distribution 口径。

## 1. M5 目标

1. 建立 owner-provisioned publisher lifecycle、内容寻址 bundle、成熟签名验证、SBOM/license/dependency 声明和 fail-closed admission。
2. 建立 plan-bound、approval-bound、CAS-backed package install/enable/update/rollback/revoke；所有贡献继续进入现有 registry，并继续受 Policy/CompetenceGate/执行前重查治理。
3. 建立 catalog quarantine：registry/catalog/README/manifest/skill text/agent profile 均为 `UntrustedData`，不能自报 publisher trust、owner command 或 permission。
4. 建立单 authority 到已授权 M4 Executor peer 的窄分发；分发只传公开 package bytes，credential/SecretRef/owner private path 永不随包流动。
5. 产出至少一个真实 loopback registry + authority + executor golden，形成可离线复核、无 secret 的 typed artifacts。
6. S1-S84、18-crate 图、93 EventKinds、M4 release receipt 和 canonical §1-26 永久不回退。

## 2. 范围判据

- **能力生态不产生行动主权**：catalog 只提供候选，publisher signature 只证明 key possession，admission 只证明 package 满足当前准入策略；permission/trust/approval/envelope 仍由 owner/authority 决定。
- **声明式首版**：M5 package 只携带结构化贡献描述和有界 UTF-8 资源，不加载 native library、WASM、安装脚本或自修改代码。结构不缺席，执行广度刻意收窄。
- **封闭依赖**：依赖必须 exact version + digest，完整 closure 在 admission 时给出；不运行递归在线 solver，不允许安装中临时从未知源补依赖。
- **不可变 release**：`(package, version, digest)` 一经 admission 不可变；update 是新 release，rollback 只切回已知良好 release，不重写历史、不撤销既有外部效果。
- **真实动作仍走咽喉点**：catalog fetch、package distribution 和任何 package 贡献触发的外部动作继续走 Harness -> Policy/Approval -> CompetenceGate -> ActionBackend -> Verification。
- **M4 联邦边界继续生效**：peer 不自装、不自选 package、不成为 publisher trust owner；远端分发必须绑定 active peer grant、epoch、plan digest、one-shot lease/fence 和 authority verification。
- **潮线判据**：搜索、推荐、依赖排序和生态发现智能在 trait 后薄建；forme 永久做深的是 provenance、admission、policy、lifecycle、ground truth、revocation 和审计。

## 3. 范围决策

| 编号 | 主题 | M5 决策 | 不进入 M5 核心的部分 |
|---|---|---|---|
| D96 | 阶段定义 | M5 是受治理能力生态；M0-M4 的 18-crate 运行时、S1-S84、93-kind prefix 和 M4 receipt 是冻结基线。 | 重做 Harness、第二个 authority、用生态广度弱化旧门。 |
| D97 | Authority | 仍只有 configured owner 和 authority Harness/EventStore 能 provision publisher、批准 install、改变 active release 或接受 distribution receipt。 | catalog/publisher/peer 自注册、自审批、自写事实。 |
| D98 | Package 模型 | `CapabilityPackageManifest` 描述 package/release/version/kind/scope/contributions/permissions/dependency closure/SBOM/license/body digest；release 内容寻址且不可变。 | 模糊 latest、可变 tag 直接激活、隐藏依赖、运行时补文件。 |
| D99 | Catalog 信任 | catalog index、描述、评分、README 与 bundle bytes 全部是 `UntrustedData`；fetch provenance 由 Harness/ActionBackend 盖章，先 quarantine 后判断。 | 搜索排名等于推荐、catalog 自报 trusted/owner、内容注入改 policy。 |
| D100 | Publisher 身份 | owner 显式 provision versioned `CapabilityPublisherGrant`，绑定 public-key digest、允许 kind/scope、TTL 和 revoke 状态；签名使用成熟 Ed25519 实现。 | TOFU、地址/域名即身份、自研密码学、签名自动生成 trust。 |
| D101 | Admission | 固定顺序验证 schema/size/closed set/path/digest/signature/publisher grant/dependencies/SBOM/license/secret markers/declared risk/managed policy；任一 unverifiable 即拒绝。 | LLM 自评 package 安全、只验签不验内容、warning 后继续。 |
| D102 | 依赖闭包 | package 依赖为 exact release + digest，拓扑无环、数量/深度有界；所有依赖先独立 admitted，根包不能扩大依赖权限。 | 在线 semver solver、隐式 transitive fetch、dependency confusion fallback。 |
| D103 | 合规物料 | 每个 release 带 license expression、SBOM digest、source/provenance 和 borrowing rule；未知/GPL-incompatible/license conflict fail closed。 | publisher 文案替代 license review、缺 NOTICE 仍 release-ready。 |
| D104 | Secret 与路径 | bundle/event/log/trace/artifact 不得含 credential、SecretRef id、private key、owner absolute path 或 private endpoint；相对资源路径需规范化且不可逃逸。 | 安装包内 `.env`、token、证书私钥、post-install secret injection。 |
| D105 | Lifecycle/CAS | canonical lifecycle 为 `Quarantined -> Admitted -> Installed -> Enabled -> Disabled|Revoked`；update/rollback 是新 change；ecosystem aggregate 使用 expected-version/CAS。 | last-write-wins、删历史、跳过 admission 直接 enabled。 |
| D106 | Install approval | install/enable/update/rollback/distribute 均形成 immutable `CapabilityInstallPlan`，绑定 release digest、贡献、scope、policy snapshot、rollback boundary；至少 L3，native/不可逆不在首版。 | “信任 publisher”后 blanket install、批后换版本/贡献/scope。 |
| D107 | Registry 接入 | admitted package 只通过现有 Provider/Capability/Plugin/Skill/MCP/AgentProfile registry 接入；disabled/revoked 贡献立即不可见，hook 永远触发执行前重查。 | package 私有执行通道、直接写 toolset、hook 覆盖 managed deny。 |
| D108 | Update/Rollback/Revoke | update 并存 staging 后原子切 active ref；失败保留旧 active；rollback 只回已知良好 release；publisher/package revoke fence 新激活与分发。 | 原地覆盖、自动 major update、rollback 宣称撤销外部副作用。 |
| D109 | Agent package | agent contribution 只是无认知 `SubagentProfile`/role template，资源最小授予，由 authority Harness spawn child run。 | 独立 memory/self-model/policy、agent team/spawn network、远端大脑。 |
| D110 | Federated distribution | 只向 active M4 Executor grant 分发 exact admitted release；绑定 peer/epoch/plan/lease/fence/digest，peer 重验 package 与本地 policy。 | peer marketplace、自动扩散、Replica 安装、分区期间自由更新。 |
| D111 | Receipt/Unknown | dispatch 后无可信 receipt 进入 unknown；同 lease 只取原 receipt/probe，不重复 install；authority 验证 digest/state/ground truth 后接受事实。 | timeout 即重发、transport 200 即 installed、通用 exactly-once 宣称。 |
| D112 | EventKind | 在 93-kind prefix 后只追加 `CapabilityPublisherChanged`、`CapabilityPackageAdmitted`、`CapabilityPackageStateChanged`、`CapabilityPackageDistributionRecorded`，总数 97。 | 为 fetch/progress/file 粒度增事实事件、重排旧 taxonomy。 |
| D113 | Artifacts | typed artifacts 至少含 publisher/admission/install/distribution/trace 五类，closed set + content address + cross-ref verifier；不得含 key/secret/private path。 | 手写 PASS、只存日志、artifact 自证 ground truth。 |
| D114 | 交付波次 | A trust/admission -> B lifecycle/registry -> C federation/golden/release；每波跑全回归并产验收报告。 | 三波无序激活、未有 admission 就做 remote install。 |
| D115 | 延后边界 | M5 保持单 owner/authority、声明式 packages、有限 registry 与静态 publisher；更广生态另立后续里程碑。 | 多租户公网 marketplace、支付/声誉、native/WASM 沙箱、硬件 attestation、自动安装/发布。 |

## 4. 三波交付范围

### M5-A：Supply-chain Trust Plane

- PublisherGrant provision/update/revoke、key digest pin、TTL 与 CAS。
- Signed declarative bundle、content address、closed dependency closure、SBOM/license/secret/path/risk checks。
- catalog quarantine + project-owned loopback registry fetch evidence。
- S85-S89、97-kind compatibility 和 M4 全回归。

### M5-B：Governed Lifecycle 与 Registry

- plan-bound owner approval、install/enable/disable/update/rollback/revoke。
- atomic active-ref/registry contribution switch；失败保留 previous known-good。
- plugin/skill/MCP/connector/agent-profile 声明式贡献接现有 registry；hook recheck、least privilege。
- S90-S94 + A/M4-M0 全回归。

### M5-C：Federated Distribution 与 Release

- admitted package 向 active Executor peer 的 plan/lease/fence-bound distribution。
- peer 重新 admission、本地 policy、receipt/unknown/recovery、revoke fence。
- authority + loopback registry + executor real golden、typed artifact set、supply-chain release audit。
- S95-S99 + S1-S94 全回归。

## 5. M5 非目标

- 多租户/多组织 marketplace、支付、公开 publisher self-service、匿名公网 control plane。
- 多主 package state、CRDT/gossip、peer-to-peer package 转发或跨 owner trust 合并。
- native dynamic library、WASM runtime、容器镜像、安装脚本、build-from-source 或任意 executable payload。
- 自动 install/update/publish、silent dependency repair、auto trust/permission/grant/envelope、standing L5。
- 独立 Agent 大脑、自主 team/spawn network、package 自带稳定记忆/策略 owner。
- publisher 信誉模型、推荐算法、通用 dependency solver、自研签名/TLS。

## 6. 成功指标

- **主权**：非 authority publisher/package state append = 0；catalog/package 生成 permission/trust/approval = 0。
- **供应链**：digest/signature/dependency/SBOM/license/secret/path 任一失败后的 install/enable = 0。
- **生命周期**：未批准或 plan 漂移的 registry mutation = 0；CAS lost update = 0；revoke 后新 activation/distribution = 0。
- **执行**：package contribution 绕过 Toolset/Policy/CompetenceGate/Harness = 0；hook 绕过 final recheck = 0。
- **联邦**：wrong peer/epoch/lease/fence/digest 的 remote install = 0；unknown outcome 盲重试 = 0。
- **回归**：S1-S84、93-kind prefix、18-crate graph、M4 artifacts/receipt、strict clippy 和 compliance 持续全绿。

## 7. M5 完成定义

1. S1-S84 全绿，`14-m5-verification-strategy.md` 的 S85-S99 全绿。
2. A/B/C 每波有 protocol compatibility note、事件序列、store/projection、反向断言、typed evidence 和验收报告。
3. 18-crate 图不新增/反向；EventKind 只末尾 additive 到 97；legacy 93-kind store 可打开且 ecosystem projection 为空。
4. real golden 经过真实 loopback HTTP catalog、Ed25519 verification、authority approval/lifecycle 和独立 executor process，且不泄漏 secret/private key/path。
5. fmt、check、strict clippy、workspace tests、Python gates、S22 doctor、dependency/license/advisory/release audit 全部 PASS。
6. 输出 `docs/acceptance/m5-acceptance-report.md`，证明真实生态能力没有削弱 M0-M4 任一治理不变量。

## 8. 下游约束

- canonical §27 必须先定义 package/publisher/admission/lifecycle/distribution 唯一口径。
- `architecture/08-m5-governed-capability-ecosystem-architecture.md` 与 `prd/22-m5-program-prd.md` 只能实现 D96-D115。
- `tools/verify-m5.ps1` 必须复跑 S1-S84、验证 93-kind exact prefix，并对当前 M5 tree 生成独立 release receipt；M4 receipt 只做历史 artifact 验证。
- 新第三方依赖必须 exact pin，登记 third-party table 与 borrowing record；只用成熟公开 API，不复制示例/fixture/错误文案。
