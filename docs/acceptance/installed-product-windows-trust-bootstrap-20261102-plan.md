# Windows 本机开发发布信任接入计划（2026-11-02）

状态：真实管理员只读预检已于 2026-11-02 13:47:44（Asia/Shanghai）通过，`product_writes=0`；`-Apply` 未执行。本文不授权服务启停、配置写入、安装或升级。

## 1. 必要性与固定输入

当前安装已认领并保存 revision 1，但 Host 未配置 `release_staging_config_path`。管理员只读 inventory 与已登录维护快照均确认签名发布入口尚未开放。不能再次运行首装 Inno 覆盖现有程序；需要先完成管理员独立预置的发布公钥配置，再通过既有 Host/updater 生命周期更新。

| 项目 | 固定值 |
|---|---|
| Product URL | `http://127.0.0.1:18765/` |
| 当前 active release | `forme-windows-x86_64-20261027-7cb5b8b` |
| 安装绑定 | `installation:969387966500ba7c649fcc449c073cd5d27042b2b2989f2eda3c7a8d5f376258` |
| 当前安全快照 | product revision 2，operations 空，config revision 1、ack 空；Host/Web Running，Brain/updater Stopped。 |
| 修复提交 | `db96ec2df073352beb1ed1a14ddc081df0796243` |
| 准备的 signed release | `C:\code\forme\target\product-bundles\forme-windows-x86_64-20261102-db96ec2-signed-local-dev` |
| release digest | `sha256:1acb2f0a400dbb78a51b4d5f84023d5239155ece704e9dcf504a47266454dbbf` |
| 本机开发 key id | `local-dev-20261029`；与仓库外独立公钥配置逐字节比对。 |
| prepared public trust SHA-256 | `6999d15220d8c19d1f0a7bbd4d02935fd33712205dbc8b936c1de46e23d64f17` |

这是 ForMe release 的 Ed25519 验签信任配置，仅限本机开发验收；不导入 Windows 根证书、不修饰旧 Authenticode UnknownError、不产生正式发布资格。

## 2. 可审查脚本与只读预检

主脚本：`C:\code\forme\target\installed-status-authorization-evidence\enable-local-release-trust.ps1`。

- SHA-256：`840fa896ad98b24c44358430215f374d3ecdabcedde8947d9c808bc7563ec122`。
- 默认不带 `-Apply`，只读核对现有五份配置摘要/ACL、四服务绑定与六程序摘要、安装身份、当前原生服务状态及受认证只读维护快照，再用当前 Rust verifier 验证具体 signed release。仅创建本机开发证据 `trust-preflight.json`，不写产品目录或 journal。
- 预检只在内存使用 Host 已配置的 Web service credential 来读取正式维护状态；不输出 credential，不读取 owner 口令/provider key，不创建或重置身份，不打开脑库。
- 申请 UAC 的只读进程不带 `-Apply`。Windows UAC 由 owner 操作；自动化不能代为确认。
- 后续 `-Apply` 必须绑定已审查 `trust-preflight.json` 的精确 SHA-256。任何配置、程序、service binding、revision、待决操作或已有输出变化都在副作用前拒绝。

配套 `trust-bootstrap-functions.ps1` SHA-256 为 `30ba352c350fd5ba0bea9acc69f62b5c17e02a0b0faf32a0ae1ef4436fc1c9bf`；实际 NTFS 临时夹具在 Windows PowerShell 5.1 和 PowerShell 7 各通过 12 项，覆盖旧字段保持、null/array、重复接入拒绝、旧摘要拒绝、保护 ACL 的新文件与原子替换、重复替换拒绝。夹具不接触已安装产品。脚本在两种 PowerShell 中语法解析通过；这些不代替真实管理员预检与 Apply 结果。

### 2.1 已完成的真实只读预检

owner 确认本次只读 UAC 后，管理员进程实际执行了不带 `-Apply` 的脚本，并生成完整的 `prepared` 回执。本机证据为 `target/installed-status-authorization-evidence/trust-preflight.json`，SHA-256 为 `d10e5a71990cf5fb64d1f15de92efaad4078938669289a07eed9c3971b39029d`；启动观察器仅保存进程 ID，未单独保存管理员进程退出码。

- 回执为 `read_only=true`、`outcome=prepared`、`product_revision=2`、`product_writes=0`，候选摘要与 §1 完全一致；实际 Rust verifier 通过签名与封闭文件集核验。
- 五份原配置、六程序摘要及四服务原绑定均已记录。Host/Web 仍 Running，Brain/updater 仍 Stopped；后续非管理员 SCM 复核观察到相同 PID 与服务状态。
- 预期 Host 补丁 SHA-256 为 `db78b6c4998955346a0c0db138a538d2b94ff2b6a9e31eaf076ba49f5bca0458`，公钥文件摘要与 §1 一致。脚本与配套函数摘要复核未变。
- `provider_keys_read=false`、`native_certificate_trust_changed=false`；未产生 `trust-apply.started` 或 `trust-apply.json`。只读 UAC 的确认不视为 §3 的写入/服务生命周期批准。

## 3. 需要独立批准的一次 Apply

只有在前述预检成功并由 owner 批准后执行一次：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\code\forme\target\installed-status-authorization-evidence\enable-local-release-trust.ps1 -Apply -PreflightSha256 d10e5a71990cf5fb64d1f15de92efaad4078938669289a07eed9c3971b39029d
```

命令必须以管理员运行，UAC 由 owner 确认；实际执行前仍核对 §2 主脚本摘要，不更换脚本、预检文件或候选。

1. 全部前置状态重查，并落本机一次性 attempt marker；原回执/marker 已存在则停止。
2. 仅停止 Web 一次；重新读取 Host 维护快照，确认无新增命令/待决；随后仅停止 Host 一次。Brain/updater 必须保持停止。
3. 用 Host 配置原有保护 ACL 保存 `C:\ProgramData\ForMe\config\host.json.before-local-dev-trust-20261102`；原件保留。
4. 创建公钥文件 `C:\ProgramData\ForMe\config\release-trust-local-dev-20261102.json`，不包含任何私钥或模型密钥。
5. 仅给 `host.json` 增加 `release_staging_config_path` 指向该文件；所有已有字段语义逐一保持，使用相邻 staging、flush、原子 replace 与摘要/ACL 回读。其他四份配置和六程序摘要保持。
6. 仅启动 Host 一次，验证同一 product revision 2、config revision 1、无 pending operation，并确认 `stage_signed_release` 能力出现；之后仅启动 Web 一次并核对四服务原绑定。

此操作不调用安装器、不替换程序、不写 release inbox、不提交 StageSignedRelease/Upgrade、不修改 active release、不启动 Brain/updater、不调用模型、不改防火墙或 Windows 证书信任。它只为下一次绑定候选的正式升级准备前置条件。

## 4. 失败与完成条件

任一步失败保存 `trust-apply.json`、已完成动作、原保护备份和未决 staging；不自动回滚、不循环重启、不删除现场、不换 operation 重做。已消费的 Apply 需要按原生验收计划 §3 单独审查具体恢复。

完成只表示：本机开发发布公钥已绑定、旧 Host/Web 恢复、原安装/账号/模型配置与程序保持、Brain/updater 仍停止。状态查询源码缺陷仍要等后续具体 signed release 升级后才能在安装实例验证。整体目标、正式签名发布、IP01–IP24 继续未闭合。
