# 受管 Web 网络配置组件验收

日期：2026-10-24。实现提交：`3de4f58`；施工分支：`shu/managed-skill-config`。承接 prd/25 §11.8–11.9；本报告是安装产品整体里程碑的组件记录，不提升 IP09/IP21/IP24，不改写历史 S/C 通过范围。§11.10–11.11 的公共设置 v2、监听检查及失败回退是随后增量，不在本提交中冒充已实现。

## 实现范围

- typed 网络设置与模型、brain、skills 共用 CAS/revision/原 operation；Host schema 6 保留旧 schema、摘要与运行配置。普通保存、core ack、恢复数据切换不激活网络。
- desired 与 published 分开；overlay 的确切 bytes/digest 进入独立产品 journal 和不可变 revision。重复保存与中断恢复保留原绑定；被篡改投影不能通过重新保存被悄悄覆盖。
- TLS/Host 校验共用协议规则，明确 IP/端口、精确 authority、材料目录和有界普通文件；私钥输入暂存使用已有 zeroize。实际 PEM/key matching 与证书指纹参与 owner operation 的 validation digest。
- Web 设置支持保存、静态检查、近期口令、应用和原号查询。投影已发布不显示为网络已验证；页面保留候选地址与未决操作，不自动重发或重新执行密码动作。
- 网络请求先持久化 Planned、关闭 HTTP 响应，再由同一受监督 Host worker 消费唯一内存 ticket。重复 POST、查询、响应失败与进程重启不生成新 ticket。执行前重查原身份、command、配置与材料；副作用前持久化 intent。
- firewall reconciliation 必须绑定旧/新规则并核实旧范围移除。Windows 原生适配只读验证管理员预配的规则及其实际生效状态，不创建、修改或删除防火墙规则；仅在已注册、身份/路径/账户/恢复策略/实际管理员权限均匹配的 Host 内宣告可用。开发程序、缺少规则、其他平台或没有 adapter 时在 plan 前拒绝。错误 receipt、未知 Web start 保留原操作。测试没有修改本机防火墙或服务。
- 首次安装增加独立的 `config/web-network` 与 `config/web-material`：管理域写入、Web 只读、Brain 不可读。Windows 32 项权限计划及 POSIX setgid 发布策略覆盖原子替换后的读取；旧安装模板不静默迁移、不扩大 config 根目录权限。
- 旧日志的可选网络摘要省略空值，独立生成的真实旧 wire/hash 记录原字节重放，与新记录混合恢复；篡改仍阻断。原全局维护互斥覆盖未决网络操作、不同新 key、重启和 RecoveryRequired，不更改既有生命周期分类。

## 验证记录

当前源码的统一质量门通过：Rust workspace/all-targets 919 passed、0 failed、5 默认 ignored；strict Clippy、格式/check、前端 TypeScript 与 68/68 行为测试、可重复嵌入资源、完整默认合规、差异空白检查均 PASS。624 源文件、63 处既有规模债务未增长；npm audit 为 0 vulnerabilities。提交时另移除一个新文件末尾空行，未改行为或生成资源。

专项通过：52 项配置测试（含 20 项网络测试及旧日志字节回归）、4 项持久维护 fence、2 项真实 TCP 回执/dispatch 顺序、2 项原生适配 Rust 单元、3 项脑偏好与 2 项受管 skills 运行时；真实 Web→Host→core 进程门的 3 项测试 PASS。Windows PowerShell 5.1/7 各自的只读 firewall 算法 fixture 25/25 与临时 NTFS 权限 8/8 PASS；POSIX 包装结构/控制流 7/7 PASS。fixture 算法与临时 NTFS 不代表实际管理员 firewall、SCM 安装或 POSIX 原生通过。

冻结门 PASS：18 crates、99 EventKinds/精确 97-kind 前缀、additive closure 字段和精确依赖锁。日志为 `target/network-configuration-{quality,process,workspace}-final-v3-20261024.log`。早期 v1 的旧 journal 兼容失败、v2 的测试工具 File.Replace 参数及路径分隔比较失败均已修复，未把旧日志当最终 PASS。未读取 `.forme`、真实 owner DB 或模型配置。

## 仍开放的产品义务

Windows 适配的实际管理员规则/SCM 证据、Linux/macOS 原生 firewall 只读核验、监听健康/失败恢复、证书分发与客户端信任、跨设备登录和完整 IP09/IP24 仍开放。未提供管理员规则生成/预览工具，不把适配代码或 fixture 当成该工具已存在。公共 v2、隔离 worker、session helper、签名升级回滚及其他清单义务继续推进；当前不是完整产品交付。
