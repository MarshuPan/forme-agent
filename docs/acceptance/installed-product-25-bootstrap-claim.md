# Web 初始化认领链路组件证据

日期：2026-10-21。分支 `shu/installed-product`，实现提交 `52808df`。本报告只覆盖本机证明、有限 setup、候选绑定和新脑目录 claim 的组件行为，不宣称完整安装包、旧脑迁移、浏览器产品或 IP01–IP24 已完成。

## 已实现

- `forme-hostd` 的维护域可以用一次性本机 rotation operation 签发 30 分钟证明；相同 operation 不重复签发，证明不写入 journal 明文。
- Web 使用独立 bootstrap credential 与维护/AgentExecution 凭据隔离；setup 路由是固定 typed allowlist，不是通用代理。
- setup journal 使用独立 OS 锁、单 writer、顺序/hash chain、先 sync 后投影和 poisoned fail-closed；截断、篡改、异安装或异签名 key 不自动修复。
- prove 创建最多 15 分钟的 setup scope；reserve/data/select/commit 都绑定 operation、setup digest、schema 和 idempotency key。浏览器不能填写 OwnerId，也不能通过 setup cookie 调用 owner API。
- 新脑候选当前仅接受已注册的空目录；目录非空、不可读、符号链接或未知数据阻断。认领提交先落 committed 事实，再签发可验证的 `SignedWebClaimCommit`；Web 只保存账户承诺，私钥/密码 hash 不跨进程发送。
- Web 进程启动时只按已持久化的同一 operation 恢复 pending claim；broker 不可达保留 pending，不新建 owner。已 committed 的同绑定重试返回同一语义，不恢复其他 setup 权限。

## 实际证据

- `cargo test -p forme-execution --test installed_bootstrap_transport`：2 项真实 hostd 进程测试通过，覆盖三凭据域隔离、错误重放、broker 被杀后的同 claim 恢复、签名验证和新 brain 目录保持为空。
- `cargo test -p forme-gateway web::bootstrap::tests`：3 项 bootstrap transport framing/typed-error 测试通过。
- `cargo test -p forme-gateway identity_setup -- --nocapture`：setup scope、候选绑定、重启 pending 恢复测试通过。
- `cargo test -p forme-gateway identity_invalid_setup -- --nocapture`：明确拒绝后的未激活 setup 清理测试通过。
- `cargo clippy -p forme-execution -p forme-gateway --all-targets -- -D warnings`：通过。
- `py -3 tools/code-quality.py` 与 `py -3 -m unittest tools.tests.test_code_quality`：通过。
- `bash tools/compliance-doctor.sh`：通过。

## 未完成

- 旧脑/备份发现、完整性/解密验证和历史 plaintext migration 尚未接入；当前不能宣称 IP13。
- 本机管理员口令恢复、typed config/secret resolver/apply/ack、Web 到 core 的正式 assertion/brain API/SSE、LAN TLS、原生服务注册、安装/升级/卸载事务和 React/TypeScript UI 尚未完成。
- 尚未取得 hostd + webd 的真实联合进程 prove→data→select→claim→login 浏览器证据；本报告不以组件测试替代该证据。

## 边界

测试只使用仓库自建 fixture 和临时目录，没有读取 `.forme` 真实脑数据、模型配置或密钥，没有创建系统账户、修改 ACL/防火墙、注册系统服务，也没有把任何 secret 写进 Git。
