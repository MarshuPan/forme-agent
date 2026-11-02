# 已安装产品状态查询修复与只读验收（2026-11-02）

本记录区分当前源码、临时真实进程与已安装实例。修复提交为 `db96ec2df073352beb1ed1a14ddc081df0796243`；它不表示已安装服务已升级，也不关闭整体目标或 IP01–IP24。

## 1. 已安装实例的实际结果

owner 已在独立 Google Chrome 完成登录。本轮使用该窗口现有会话，从地址栏访问正式只读 API；未提取 cookie、代填口令或读取模型密钥，随后已回到 Product URL `http://127.0.0.1:18765/`。

| 查询 | 实测结果 |
|---|---|
| `/api/v1/product/config?schema_version=2` | `configured=true`，`active_revision=1`，`acknowledged_revision=null`，`pending_restart=true`，`brain_source=managed_configuration`。 |
| `/api/v1/product/status` | `ConfigurationRejected`，`retryable=false`；安全文案为“产品配置没有被后台接受”。 |
| `/api/v1/product/maintenance` | `product_revision=2`，`operations=[]`，主脑 stopped，active release 为 `forme-windows-x86_64-20261027-7cb5b8b`；未提供签名发布暂存操作。 |
| Windows SCM | Host/Web Running/Automatic；Brain/updater Stopped/Manual、ExitCode 1077。 |

配置已经保存；ack 缺失表示尚无主脑加载确认。状态接口的泛化错误不能证明 API key 无效，更不能证明真实 provider 调用失败。

管理员只读检查进一步确认：Host 与 installation 的安装、平台和初始 release 绑定一致；`host.json` 中省略了可选 `release_staging_config_path`，即尚无已安装发布信任配置。本轮只记录配置摘要/字段和服务元数据，不打开脑库、不读取模型密钥、不修改 ACL、配置、程序或服务生命周期。

第一次只读检查脚本未兼容旧配置省略可选字段，停止于属性读取；修正后的 v2 记录为 `outcome=inspected, product_writes=0`。原失败回执保留，不记为产品修改。

owner 随后确认管理员只读预检 UAC，默认不带 `-Apply` 的发布信任准备脚本实际生成完整回执；启动观察器未单独保存管理员进程退出码。`trust-preflight.json` 为 `outcome=prepared`、`read_only=true`、`product_writes=0`，SHA-256 为 `d10e5a71990cf5fb64d1f15de92efaad4078938669289a07eed9c3971b39029d`；五份配置、六程序及四服务绑定核对通过，未读取 provider key 或修改证书信任。Host/Web PID 和运行状态保持，Brain/updater 仍停止，Apply marker/回执均不存在。详细绑定见[接入计划 §2.1](installed-product-windows-trust-bootstrap-20261102-plan.md#21-已完成的真实只读预检)。

## 2. 根因与修复范围

`LocalConfigurationClient::status()` 用 Web 专用配置凭据访问 Host 的 `/v1/host/status`。HTTP 层已正确认证该身份，但 `HostManagement::status()` 又只验证 LocalAdministrator 维护凭据，合法 Web 查询因此被拒绝。维护快照使用同一个安全状态投影的内部路径，所以它仍能返回上述配置版本和 stopped 状态。

修复保持原 trait、路由、DTO、crate 图和 EventKind：只读 `status()` 按实际 actor 验证既有 Web 配置身份或本机管理员身份，然后读取同一个 `status_for()` 投影。管理员的 `plan/execute/operation` 权限不扩大；AgentExecution 和其他实例的 Web proof 继续拒绝。

新增两项命名回归，纳入 `tools/verify-installed-host.ps1`：

- 合法 Web 配置身份与管理员读到相同状态；多次读取不改 journal、不执行服务动作；Web 不能提交管理员 plan、execute 或 operation 查询。
- AgentExecution 与外实例 proof 在服务观察前被拒；观察次数、服务动作次数和 journal 变化均为零。

真实进程测试同时覆盖匿名状态读取 401、认领后配置 revision 0、保存后 revision 1、ack 尚空；状态读取不增加产品 journal 长度，也不调用计数模型。Windows journal 保持原跨进程独占锁；Rust 测试 drop 后比较完整字节，进程测试只观察文件长度，没有为测试解除锁。

测试先在未修复源码上得到 `1 failed / 1 passed`，失败点明确为合法 Web 状态读取的授权拒绝；应用修复后 `2 passed / 0 failed`。

## 3. 当前源码与组件验证

| 执行门 | 结果与边界 |
|---|---|
| `tools/verify-code-quality.ps1` | PASS；Rust `1112 passed / 0 failed / 15 ignored`，fmt、workspace check、strict Clippy、778 文件规模/依赖/unsafe 棘轮、前端 96 项、npm audit、资源重现、bundle/签名/原生权限与完整合规门通过。Inno 原生 exit 夹具未提供工具链参数，按脚本为 NOT_RUN。 |
| `tools/verify-product-configuration.ps1` | PASS；80 个配置测试、相关网络/偏好/skills 与 4 个真实子进程/HTTP 场景通过；并非已安装 SCM 或真实 provider 验收。 |
| `tools/verify-installed-host.ps1` | PASS；222 个命名场景逐一执行，包括新增身份正反测试；脚本明确 `IP01-IP24: NOT_RUN`。 |
| release 构建 | `cargo build --release --locked --workspace` PASS。 |
| 当前源码 M5 release audit | PASS；独立 verify PASS，回执摘要 `sha256:23dbd686b93a06413df0afdc29b5d89208f61bab0701184c00b119072b1666f3`。这不等同重新宣告完整 M5 脚本/历史回执比较或原生产品通过。 |

日志保存在本机 ignored `target/installed-status-authorization-evidence/`：`code-quality.log`、`product-configuration.log`、`installed-host.log`、`release-build.log`、`m5-release-audit.log`。`browser-observation.json` 和 `protected-installation-v2.json` 分别保留脱敏浏览器与管理员只读证据。

## 4. 修正版发布候选与后续边界

新 signed release 为 `target/product-bundles/forme-windows-x86_64-20261102-db96ec2-signed-local-dev`，release id `20261102-db96ec2`，完整集合 18 文件。使用 owner 已选择的独立本机开发 key id `local-dev-20261029`；私钥由原本机 signer 在仓库外读取，没有复制或输出。

当前 Rust verifier 使用实际安装绑定对应的独立公钥配置返回 `signature_and_closed_content`，release digest 为 `sha256:1acb2f0a400dbb78a51b4d5f84023d5239155ece704e9dcf504a47266454dbbf`。兼容范围明确为 config 1、data 10、minimum upgrader 1；实际安装数据状态仍须由后续受保护生命周期预检核对，不能用声明替代观测。

owner 要求继续并确认本次管理员 UAC 后，[信任接入计划 §5](installed-product-windows-trust-bootstrap-20261102-plan.md#5-已执行的一次-apply)的一次 Apply 已成功执行，实际进程退出码 `0`，回执为 `trust_prepared_only`。原 Host 配置已保护备份，公钥路径已绑定，Host/Web 各停启一次后恢复，原程序和其他四配置保持；Brain/updater 仍停止，未调用真实 provider。候选尚未进入 release inbox，active release 未切换；状态查询修复仍待具体程序升级。正式升级、Brain 自动启动、配置 ack、真实对话/worker/helper 及其他 IP 仍开放。

后续入库预检在复制前停止。独立管理员只读诊断已核对：当前 core 配置指向的 store 文件和内容 key 目录尚未生成，owner/installation 绑定有效，服务状态与维护 revision 保持；未发生产品写入、维护命令或 provider 调用。不能把现有数据身份检查失败解释成 API key 无效；当前下一项为正式维护入口的首次 Brain 启动与原 operation/readiness 核实。固定诊断回执及后续顺序见[升级计划 §5–6](installed-product-windows-upgrade-20261102-db96ec2-plan.md#51-已完成的只读定位)。
