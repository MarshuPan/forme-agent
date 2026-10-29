# Windows 本地开发签名回执（2026-10-30）

本回执记录当前 `main` 提交对应的 Windows release bundle、仓库外的本地开发签名和离线验签。`local-dev` 信任根只用于本机受控验收，不是正式发布信任根。本次没有安装、升级、替换服务文件、修改现有安装配置或启动 Brain。

## 1. Release 材料

| 项目 | 结果 |
|---|---|
| 源码提交 | `566dfe7e9632a39af42a518a94ae8fe0d2912cb3` |
| bundle | `target/product-bundles/forme-windows-x86_64-20261030-566dfe7` |
| release id | `20261030-566dfe7` |
| target | `windows_x86_64` |
| bundle 文件数 | `18` |
| bundle content digest | `sha256:2ec417469b94d0ddcb386f1b6022d6f172ec7ff367152b891fbeabaedb1c71f0` |
| signed release | `target/product-bundles/forme-windows-x86_64-20261030-566dfe7-signed-local-dev-20261030` |
| signed release digest | `sha256:b0948f15f1bc7341dd4f89141c046d5bc2e58ef15844bf07778c7524c9fe96ce` |
| key id | `local-dev-20261029` |
| compatibility | config `1..1`，data `10..10`，minimum upgrader `1` |
| native package signature | `not_performed` |
| native installation | `not_performed` |

签名私钥位于仓库外的 `C:\Users\shu\AppData\Local\forme\dev-signing\release-ed25519.pem`，没有复制到 bundle、源码或验收回执。管理员信任配置位于同目录的 `release-trust-local-dev.json`，其 SHA-256 为 `b75d2326c7f8f19753ab319e2efbadede2691be2a0bff09282b84f686775f64`。

## 2. 构建与验签

- `cargo build --release --locked --workspace`：PASS；构建输入为上述提交。
- `node tools/build-product-bundle.mjs`：PASS；输出为新目录，未覆盖旧 bundle。
- `node tools/sign-product-release.mjs`：PASS；签名输出为新目录。
- 当前提交构建的 `forme-hostd --verify-release`：PASS，exit code `0`。

```text
verification: signature_and_closed_content
release_digest: sha256:b0948f15f1bc7341dd4f89141c046d5bc2e58ef15844bf07778c7524c9fe96ce
```

签名目录包含闭合的 `content/`、`manifest.json`、`signature.ed25519` 和 `signing-receipt.json`。对签名输出的敏感材料扫描为 clean；未发现模型 API key、Bearer token 或私钥 PEM。

## 3. 未关闭的边界

本回执不代表：

- Authenticode 签名、正式发布信任根或正式发布包；
- Windows 原生安装、SCM 注册、ACL/账户和服务 token 验收；
- Upgrade/Rollback、服务恢复、BrainReady 或 IP01–IP24；
- 当前已安装实例已经切换到这个 release。

后续任何安装或升级仍须通过受保护 Host 控制面完成真实 installation binding、owner 重新认证、exact operation 和独立的原生验收。
