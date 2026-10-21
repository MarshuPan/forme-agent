# Web 独立认证基础组件证据

日期：2026-10-21。实现分支 `shu/installed-product`，源码提交 `12b3f5d`；维护规范 `3236e50`、质量门 `b64fc04`/`b3b779f`、Gateway 职责拆分 `b373aec`。这些提交尚未合入 main，不是完整产品发布或 IP06 PASS。

## 已实现与边界

`gateway::web::identity` 使用自己的 OS writer 锁与 SQLite FULL/WAL 事务，仅保存安装绑定、准备账号、单 owner、密码哈希、session/reauth 摘要。未知用途/版本、其他安装/签发公钥、另一 writer 或损坏文件均拒绝，不自动修复覆盖。

账号准备与 broker 已提交回执分开；`WebClaimCommit` 的固定公钥签名绑定 installation、operation、verified candidate、真实 owner、随机盐账号承诺和期限。原 operation 可幂等收尾，不能替换 owner。生产 broker 的 setup、旧脑/恢复验证、签发和双边崩溃恢复尚未接入；只有自建 fixture 签发测试回执，没有匿名注册路由。

密码采用 Argon2id v19，64 MiB/t3/p1、16-byte OS salt、32-byte 输出；参数封闭且最多两个密码计算。密码和原始 cookie 不派生 Debug/Serialize，短期秘密使用 Zeroizing。零化不保证消除 OS/runtime 的所有副本。

session 使用 256-bit OS 随机值，仅存摘要，idle 30 分钟、absolute 12 小时。CSRF 使用独立域 nonce，mutation 仍须后续 HTTP 的 Host/Origin 校验。重新认证 5 分钟、绑定具体 session/epoch/operation 且单次消费；口令变更绑定预先准备的新哈希承诺。epoch/hash/proof 消耗/撤销在同一事务提交。生产时钟在取得认证库锁后读取，慢 KDF 后再次检查会话有效性。

## 本次验证

对上述源码实际执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`。
- `cargo test --workspace --all-targets`：678 passed、0 failed、5 默认 ignored。此计数不是 5 个 ignored 已执行的证据。
- `cargo clippy --workspace --all-targets -- -D warnings`、`bash tools/compliance-doctor.sh`。
- `py -3 -m unittest tools.tests.test_code_quality`：4 项；`py -3 tools/code-quality.py`：326 个源文件、66 项历史规模债务，PASS。
- `tools/verify-web-identity.ps1`：12 个命名测试各执行一次；包括认领签名/承诺/期限、重启、CSRF/TTL/时钟回退、reauth 单次/跨 session/epoch、口令替换绑定、外库/双 writer 拒绝及 8 线程共用 session。
- `tools/verify-installed-host.ps1`：宿主组件与 core 停服门 PASS；`git diff --check` 通过。

可重复的门脚本已提交；本机详细日志在忽略的 `target/product-auth-*.log`、`target/product-identity-gate.log`。本报告不以修改前的测试替代修改后源码验证。

依赖检查中，新引入的 argon2/zeroize 未出现安全通告；`cargo audit` **不是全绿**。仍有既有 quick-xml 0.30.0/0.39.4 的 RUSTSEC-2026-0194/0195，以及 zbus_names/zvariant/zvariant_derive 的维护告警。当前 Windows GNU 目标的反向依赖查询均不可达；Linux/macOS 发布前仍须处理各目标实际可达性，不将 Windows 结论外推。许可证/NOTICE 已同步，无新增内部依赖边或脑 EventKind。

## 未验收内容

本次没有创建 OS 服务/账户、修改 ACL/防火墙、读取 `.forme` 的个人配置或真实脑数据，也没有导入 DeepSeek 前端。

`forme-webd`、认领 HTTP、本机口令恢复、typed config/secret resolver、core 会话 assertion、LAN TLS、原生安装包和五面 UI 尚待后续实现/接入。IP01–IP24 为 NOT_RUN；完整 S1–S99 永久脚本/clean-tree release receipt 另收，不能由本组件报告宣称通过。
