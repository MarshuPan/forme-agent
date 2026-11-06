# Windows 测试实例重置与重新安装（2026-11-06）

## 1. Owner 指令与范围

Owner 明确要求先处理旧升级、继续在本机测试，随后明确授权：“所有东西都可以重置，不需要留任何东西”。本记录据此替代旧安装的保全/迁移路线；不需要虚拟机或继续收集缺失的历史来源。这是测试环境销毁与重新部署，不是将旧升级判定为成功、安全回滚或 Aborted；产品恢复协议不变。

授权清除的唯一实例：`installation:969387966500ba7c649fcc449c073cd5d27042b2b2989f2eda3c7a8d5f376258`。移除范围：

- `C:\Program Files\ForMe`：旧程序、静态 UI、安装器卸载文件。
- `C:\ProgramData\ForMe`：旧账号、keys、脑数据、配置、工作空间、备份、日志及卡住的升级 journal；不备份，不自动迁移。
- 四个固定 SCM 服务：`forme-web`、`forme-brain`、`forme-release-updater`、`forme-host`。
- 对应该安装的唯一 Inno 卸载登记：`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{9F0D7A2B-5BF8-4A6F-9AC1-5A2D3F4E6B7C}_is1`。

仓库、工作树、Git、源码验收工件、仓库 `.forme`、外部项目、其他程序和本地开发签名材料不在清除范围内。模型配置随旧产品数据一起清除；新实例需重新认领/配置，不沿用旧安装身份。删除不提供恢复副本。

本轮重置、重新安装及限定新测试实例上的必要诊断/服务停启/测试已有 owner 授权，不逐次重复索取同类操作确认。Windows 管理员令牌仍由 OS 提供；这不授权重置 OS、其他软件/项目或更改冻结产品协议。

## 2. 固定候选

- 源码：`d22b6a35af703015722f0a733c3af5cc20499241`；tree：`2b86513fad918603110a7f968286e1cfcf9ba9ba`。
- release：`forme-windows-x86_64-20261106-d22b6a3`。
- 安装器：本工作树 `target/product-bundles/forme-windows-x86_64-20261106-d22b6a3-package/artifacts/forme-0.0.1-windows-x86_64-development-signed.exe`。
- bytes：76,504,768；SHA-256：`73ac983880f8c9274f52f2ad7db4f5c86d5faaa2e748cde170e0d805f7b0a63a`。
- 本地开发 Authenticode 证书：`F70FE25CA2846493F7E236D41C8DD566CB71AB3C`；系统链信任仍不可验证，不安装系统信任根、不宣称正式发行。
- Ed25519 闭合包验证已通过；源码供应链审计及独立 verify 均通过，artifact `sha256:0865d1aa056b6170a682ae5621b6fbea90a33a78fff56b0245957429c8e8cf03`。这不是新一轮完整 M5/IP 验收。

## 3. 一次管理员执行窗口

固定脚本：`target/native-legacy-review-20261106/reset-local-test-installation.ps1 -Apply`。

1. 核对安装身份、四服务完整 image/账号、卸载登记、绝对固定根和所有子项无 reparse point；记录数量与字节，不读取账号口令、密钥或脑正文。重新核对安装器摘要与签名证书。
2. 禁用四个旧服务的启动，停止 Web/Brain/updater/Host，验证实际停止；处理仅位于固定安装 bin 下的旧产品进程。其他服务/应用不受影响。
3. 删除四个 SCM 登记，确认消失；重新核对固定根与子项后，删除两个确切根及唯一卸载登记。保留最小操作回执，不保全旧产品数据。
4. 核对 fresh-install 前提后运行上述唯一安装器一次，使用安装器自己的 Prepare/Register/Grant/Verify/Start。非零退出、超时或未知均停止，不自动重跑。
5. 独立核对新安装身份、release、六程序摘要、四个 SCM 登记/状态及 Product URL。首次安装只启动 Host/Web，Brain 等待认领与模型配置。接着沿正式产品接口完成初始化与真实运行验收。

脚本默认仅检查；`-Apply` 才执行上述窗口。当前 Codex 不是管理员，Windows 可能要求一次 UAC；这只是取得 OS 管理员令牌，不重新索取上述操作授权。不自动重启/注销 OS，不修改系统防火墙、证书存储或外部项目 ACL。

## 4. 实际结果

已执行。2026-11-06 14:06:43–14:07:04（Asia/Shanghai），管理员脚本核对旧安装身份、四服务、卸载登记及无链接目录后，清除旧程序根 36 文件 / 837,458,016 bytes、旧数据根 68 文件 / 419,601,057 bytes，移除四个服务及唯一卸载登记。没有保留旧产品数据副本；仓库、外部项目与既有验收工件保持。随后唯一固定安装器执行一次，exit 0，无 OS reboot；Prepare/Register/Grant/Verify/Start 全完成，六个安装程序摘要与候选逐一匹配。

新 installation：`installation:0d87010c913c2299385d98cbf952696c078834adaa52147126e42a626be222a1`。首次 Host/Web Running、Brain/updater Stopped，Product URL 返回初始化入口。14:11:40–14:11:41，使用已有本地测试账号/模型配置，经本机维护证明与正式 Web setup/config API 完成新脑认领、配置；没有输出 proof、口令或 key。保存模型后自动 Brain activation Committed，active/acknowledged revision 均 1，pending_restart=false，product/core ready。没有手动 start Brain 替代产品自动启动。

随后经正式 Web 登录、持久会话、`brain/runs` 与 ProductionRuntime 完成真实 provider `compatible:gpt-6.1-sol` 调用：原 run `run:1793945595483167200:1` Complete，回答匹配测试标记；stream_seq 1–19 连续，包含 RunAccepted、SessionBound、GoalFramed、ResourcePlanned、DoneContractSet、AutonomyEnvelopeSet、DecisionTrace、ContextBuild、ModelCallStarted/Finished、Verification、TurnComplete 与 RunComplete。同一 command 重提返回同 run，零新增模型调用。

首份 smoke 的负向 fixture 在 PowerShell WebRequestSession 中继承了先前 CSRF header，实际送出有 CSRF 的空 body，故收 400 并将报告记为 stopped；没有改成 PASS，也没有为此重跑模型。独立补充脚本使用只共享 cookie、完全新建 headers 的客户端重新验证：匿名 401、错误 Origin 403、缺 CSRF 403，run 数量 1→1，原 run 事件仍 19。两份证据联合支持该 smoke 与入口拒绝结论，不代表整个 IP24。

14:15–14:18 的原生服务窗口另已闭合：Brain 停止期间 Web 仍可认证读取维护状态；停止 Host 时 Windows 根据 Web→Host 依赖同时停止 Web。初版脚本只 start Host 后即等待网页，因遗漏 dependent Web start 而超时，保留原失败回执。没有重跑整段；补充脚本核对 Host/Brain 已换 PID、Brain 已由 Host 自动恢复后，只启动原 Web，再执行一次明确 Web restart。原登录 session 跨 Web restart 继续有效，owner/store identity 不变，原回答保留、原 run 仍 19 个事件，最终 Host/Web/Brain Running、updater Stopped，product/core ready。Web 达到 SCM Running 后首个 HTTP 200 实测为 63 ms；这是单次服务窗口观察，不是 OS 冷启动性能保证。

本轮从正式安装包完成的范围是新安装、认领/配置、自动启动、真实对话/幂等及部分安全反例。HTTP/API 证据不替代浏览器交互；完整 ACL/隔离副作用、桌面 helper、升级/恢复、OS 冷启动/注销与卸载等仍按 IP 单独登记。runtime doctor 仍 blocking（notification worker 未绑定）；其他 disabled/unsupported 消费者不得以产品 ready 掩盖。开发签名不等于正式发行，Linux/macOS 原有限策略/不发布边界保持。

## 5. 最小验收工件

工件均位于本工作树 `target/native-legacy-review-20261106/`，不包含口令、cookies、proof 或 provider key。

| 工件 | SHA-256 | 范围 |
|---|---|---|
| `local-reset-result.json` | `99ae3dea5eb0a60ec5474644c2f969213956999e1dc22344d25b6a59d480f9fd` | 原安装清除、新安装及六程序/SCM/HTTP 观察 |
| `fresh-install.log` | `037251b0c47be667ef353ac056dc22dad92455ba4b4f31d7d3c8a74d55bd0cbc` | 唯一安装器的实际阶段 |
| `fresh-initialization-result.json` | `9ef6ebb3ee94cbc6059615a6482a226bbdabc06f6e39b75248f1981a76986def` | 正式 setup/config、自动 Brain 启动和 ack |
| `installed-product-smoke-result.json` | `017dd4afc525744503dcc083e13a91fb19311b041266fdd720f3b610afb27674` | 真实模型/连续事件/幂等；保留负向 fixture 失败 |
| `installed-admission-corrected-result.json` | `f4362f375b25f716c02ef8273c30937019e21fe2e6c6a742ee6ed69a471eaef3` | 独立 header 状态下的反例，零新增 run |
| `installed-service-cycle-result.json` | `2986ec888d56ddacb72a178e8f0bf23896e7b75e569b77ad4cd524ee72d60ba4` | 保留初版脚本遗漏 dependent Web start 的超时 |
| `installed-service-cycle-completion.json` | `4caa2179283621d67a89ad00a92b1fe43f95cbc969b7ca9832ce1c246e98834f` | 延续同一服务窗口，自动 Brain 恢复与会话/脑/原结果保留 |
