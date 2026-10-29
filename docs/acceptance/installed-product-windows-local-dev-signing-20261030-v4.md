# Windows 本地开发签名与原生候选回执（2026-10-30，v4）

本回执记录本机受控验收用的 Authenticode 候选。它不是正式发布信任，不代表已安装、已升级或 IP01–IP24 已通过。私钥只保存在当前用户证书库；本回执、仓库和安装包均不包含私钥。

## 1. 输入与候选

产品代码输入为 `main` 的 `433039b`。当前 `main` 为 `463348a`；两者之间只有验收文档变化，未改变产品代码或打包输入。候选使用现有 Windows bundle 重新复制并签名，没有执行安装器。

| 项目 | 结果 |
|---|---|
| unsigned installer | `target/product-bundles/forme-windows-x86_64-20261030-433039b-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| unsigned SHA-256 | `sha256:dec05d1941366c65b2ba8b2257826877a7bea3c1fff6d34fa6b0252585a3ffcf` |
| local signed installer | `forme-0.0.1-windows-x86_64-local-dev-v4.exe` |
| signed SHA-256 | `sha256:9ee9ebf86fab1ea7f17ea91c46f57a6d3787b36984033df03cbeace3fb97c571` |
| signed bytes | `73,577,840` |
| native installation | `not_performed` |
| release ready | `false` |

## 2. 本地开发证书链

本次使用正确的本地开发链：独立 Root CA 签发代码签名 Leaf。Root 和 Leaf 的公钥证书仅作为本机验收材料导出到 ignored `target` 工件目录；私钥保留在 `CurrentUser\\My`。

| 项目 | 结果 |
|---|---|
| Root subject | `CN=ForMe Local Development Package Root 20261030 v4` |
| Root thumbprint | `84307392F23C57B4290179CA6546ECE73AD3E1D6` |
| Root validity | `2026-10-30 06:43:40` – `2026-11-29 06:53:40` |
| Leaf subject | `CN=ForMe Local Development Package Signing 20261030 v4` |
| Leaf thumbprint | `F70FE25CA2846493F7E236D41C8DD566CB71AB3C` |
| Leaf issuer | `CN=ForMe Local Development Package Root 20261030 v4` |
| Leaf validity | `2026-10-30 06:43:40` – `2026-11-29 06:53:41` |
| custom-root chain verification | `valid` |

自定义 Root trust 验证确认 Leaf 的签发关系、用途和有效期正确。该验证不等于 Windows 全局或当前用户信任提供程序已接受它。

## 3. Windows 信任状态

`Get-AuthenticodeSignature` 对 v4 安装器返回：

```text
SignatureType: Authenticode
Status: UnknownError
StatusMessage: A certificate chain processed, but terminated in a root certificate which is not trusted by the trust provider
```

`CurrentUser\\Root` 中没有 v4 Root。使用 PowerShell `Import-Certificate`、`certutil -user -addstore` 和 .NET `X509Store.Add` 的当前用户导入尝试均未完成写入，其中挂起的进程已停止；没有修改 LocalMachine 证书库，也没有改变服务或产品配置。因此签名工具回执保持 `authenticode_present_unverifiable`，不能把该候选称为可信发布包。

## 4. 验证与未关闭边界

- `node --test tools/tests/windows_package_signature.test.mjs`：`3 passed / 0 failed / 1 skipped`。
- `tools/sign-windows-package.mjs` 独立读取签名结果、签名者指纹和输出摘要；签名输出回执已绑定 v4 文件。
- 未运行 Inno 安装器，未写入 release inbox，未切换 active release，未停止或启动服务，未启动 Brain，未修改 owner 数据、配置或密钥。
- 当前结果只关闭“本机开发 Leaf 签名候选生成与自定义链核对”；不关闭 Windows 原生安装/升级、IP01–IP24、正式发布签名或完整产品验收。

后续安装或升级仍必须由受保护 Host 读取真实 installation binding、active release、schema 和 trust 配置，绑定精确候选摘要并取得 owner 的一次性 operation 确认。不能因为本地签名存在或自定义链有效而直接运行安装器。
