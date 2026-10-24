# 受管 Skills 配置与运行时接入组件验收

## 1. 范围与结论

- 设计基线：`docs/prd/25-bootstrap-auth-and-web-access.md` §11.7，以及 `canonical-contract.md` §29 的产品配置边界。
- 实现提交：`b50a7d7`；对应设计提交：`ae0de84`。
- 组件范围：`ProductCapabilitySettings` / `ProductSkillConfig` / `ProductSkillTrust`、配置事务持久化、Host schema 5、正式 `SkillRegistry` 装配、Web 设置编辑和重启/恢复保留。
- 结论：**组件实现与组件回归 PASS**。这只关闭“受管 skills 接入”这一项，不等于 MCP、plugin、package、消息渠道、原生安装、真实 provider 或 IP08/IP21/IP24 完成。

## 2. 实现边界

- skills 与模型、brain preference 共用同一 `ProductConfigPatch`、revision、operation、journal、CAS 和恢复语义；缺省字段保留现值，显式空列表才清空。
- 配置对象是 closed/versioned；限制 schema、数量、正文/摘要/总配置大小、重复 ID、控制字符、空正文、非法 scope 和未知字段/版本。
- disabled、untrusted、越 workspace scope 的项不会进入正式 registry 可见集合或模型 context；可信、启用且同 scope 的项仍按现有 registry、context 和 Harness 治理按需加载。
- 配置不得声明 shell、MCP、browser、plugin executable、任意环境变量或权限；skills 不是执行授权，也不能替代 `AutonomyEnvelope`、Policy、Approval 或 CompetenceGate。
- 保存/恢复结果不明时保留完整原始 operation 和配置，不自动重发局部 skill；旧 schema、旧 digest、旧客户端保存和离线数据投影保持兼容。
- Web 只提交/展示受认证配置 DTO；不把 skill 当作密钥输入，不旁路 core，不创建第二脑或第二 memory 事实源。

## 3. 验证证据

以下验证均在隔离实现工作区执行，未读取真实 `.forme` 数据或真实密钥：

- `cargo test --workspace --all-targets --locked`：PASS，包含 managed configuration、正式 ProductionRuntime、Web、C1–C24 和 M0–M5 回归；Gateway 87 个单测复跑为 87/87 PASS。
- `cargo test --locked -p forme-execution --lib installed::product_config`：31/31 PASS。
- `cargo test --locked -p forme-harness --test installed_brain_preferences`：3/3 PASS。
- `cargo test --locked -p forme-harness --test installed_managed_skills`：2/2 PASS；验证可信/启用/同 scope 选择、禁用/不可信/越 scope 拒绝，以及无 provider 调用的失败闭环。
- `tools/verify-product-configuration.ps1`：PASS；真实 Host/Web/core 进程链验证配置保存、重启、正式 context 正文、workspace 隔离和旧配置兼容。
- `npm --prefix frontend run verify`：PASS；TypeScript、Web 行为测试、静态资源生成和嵌入校验通过。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`、`py -3 tools/code-quality.py --json`、`bash tools/compliance-doctor.sh`、`cargo fmt --all -- --check` 和 `git diff --check`：PASS。

## 4. 未覆盖项

- 未执行真实外部 provider、真实 MCP/plugin/package 生命周期、真实消息渠道或原生服务账户/worker sandbox。
- 未执行管理员安装、系统服务注册、Linux/macOS 原生包、签名/公证、LAN TLS 或 IP01–IP24 联合验收。
- 因此本组件只更新产品配置的 skills 子集状态；整体独立部署/Web 产品里程碑继续保持未完成。
