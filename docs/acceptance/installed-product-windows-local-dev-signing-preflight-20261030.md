# Windows 本地开发签名与升级前只读预检回执

日期：2026-10-30

本回执记录本地开发签名材料和当前 Windows 安装实例的只读预检。它不授权安装、服务停止、文件替换、配置写入、Brain 启动或发布；`local-dev` 信任根只用于本机受控验收，不是正式发布信任根。

## 1. 签名材料

| 项目 | 结果 |
|---|---|
| 签名 release | `target/product-bundles/forme-windows-x86_64-20261029-6a18196-signed-local-dev-20261029` |
| release id | `20261029-6a18196` |
| target | `windows_x86_64` |
| content 文件数 | `18` |
| signed release digest | `sha256:987a758ec4abbe258eaf51dca5117d6de0244b94017678c94cdf5ccb6f123fed` |
| key id | `local-dev-20261029` |
| compatibility | config `1..1`，data `10..10`，minimum upgrader `1` |
| native package signature | `not_performed` |
| native installation | `not_performed` |

签名私钥与开发信任配置均位于仓库外的用户目录。信任配置使用独立的本地预检 installation ref，不代表已安装实例的真实 installation ref，也没有写入受保护的 Host 配置。

信任配置脱敏摘要：

- 文件名：`release-trust-local-dev.json`
- SHA-256：`b75d2326c7f8f19753ab319e2efbadede2691be2a0bff09282b84f686775f64`
- key id：`local-dev-20261029`
- private key：未读取、未写入仓库、未写入 bundle、未输出

## 2. 当前源码验签

使用当前 `main` 工作树重新构建 `forme-hostd` 后执行同一 Rust verifier：

```text
forme-hostd --verify-release <signed-release> --trust-config <local-dev-trust> --target windows_x86_64
exit code: 0
verification: signature_and_closed_content
release_digest: sha256:987a758ec4abbe258eaf51dca5117d6de0244b94017678c94cdf5ccb6f123fed
```

`node --test tools/tests/signed_release.test.mjs`：`5 passed / 0 failed`。测试同时确认替换签名内容会被 Rust verifier 拒绝。

当前 `main` 与签名候选的代码输入之间只存在验收文档和 M5 回执差异；本次签名 envelope 本身仍按其固定 release id 和 digest 识别，不把文档差异改写成新的二进制 release。

## 3. 已安装实例只读快照

当前 Product URL：`http://127.0.0.1:18765/`

| 服务 | 状态 | 启动类型 | 账户 |
|---|---|---|---|
| `forme-host` | `Running` | `Automatic` | `LocalSystem` |
| `forme-web` | `Running` | `Automatic` | `NT SERVICE\\forme-web` |
| `forme-brain` | `Stopped` | `Manual` | `NT SERVICE\\forme-brain` |
| `forme-release-updater` | `Stopped` | `Manual` | `LocalSystem` |

Host/Web 当前仍监听产品地址及内部 loopback 端口。服务 image 指向已安装目录的旧实例路径；本次没有替换文件或改变服务注册。

普通开发会话可以读取 SCM 状态，但不能读取 `C:\ProgramData\ForMe` 下受保护的 Host 配置、安装绑定、实际 active release、schema 或 trust 配置。由于这些值不能安全确认，本回执不猜测它们，也不把本地预检 installation ref 当成真实安装绑定。

## 4. 结论与后续边界

本回执关闭：

- 当前签名 envelope 的离线生成准备；
- 当前源码 Rust verifier 的签名、闭合文件集和内容替换拒绝复核；
- 本机四个服务的只读状态采集。

本回执没有关闭：

- 签名 release 进入真实 installation 的 `release-inbox`；
- owner 重新认证和 exact upgrade operation；
- installation ref、active release、实际 schema 与兼容性确认；
- 排空、备份、active release 切换、Host/Web 健康恢复和 BrainReady；
- Windows 原生升级/回滚、IP14 或 IP01–IP24。

下一次升级必须先在受保护 Host 控制面完成真实 installation 绑定和只读预检，再针对该 release digest 取得新的明确 operation 确认。不能再次运行新安装器覆盖现有安装根，也不能把 receipt 中的公钥自动当作已安装实例的信任根。
