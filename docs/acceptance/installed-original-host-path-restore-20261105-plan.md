# 原 Host 路径表示恢复与限定启动计划（2026-11-05）

状态：2026-11-06 owner 在上轮明确列出两项下一步后指示“继续下一步”，本固定计划据此执行一次 `-Apply`，Host/Web 已恢复；结果见[实际恢复记录](installed-original-host-path-restore-20261106-result.md)。此前 21:44 的管理员只读预检未取得管理员进程，仍不计通过。固定前置核验与 Apply 位于同一次 UAC，程序/config 原字节保留；本次不更换程序、不升级业务版本、不裁决历史 Upgrade，不表示 IP 通过，也不重复执行同一脚本。

## 1. 已核实的两个问题

原件采样脚本把 owner 写成 `owner:shu`，但实际安装使用系统生成的 owner ID。2026-11-02 原正式 intent/result 的原字节摘要分别为 `757d82e033314face57ca5d4ac7efa0f28857bdb9a6806940a9c13eff8c45c8d`、`982e289ed80e2ce6512385cf9bf3b663f30a45195f6405bd613496e3a6ca334d`；本轮重新核对一致。21:36 的真实只读 inspector 对照这两个回执后，14 项状态/水位/摘要检查和五项身份绑定全部通过，原 journal/六程序/六配置/ACL/SCM/worker 保持，服务动作为零。它没有生成可执行历史审查或恢复授权。

另一个已确认的阻断是旧 Host 的 SCM 命令匹配。已安装 Host 仍为先前核对的 `694b6bb`；该版本 `registration::matches_config` 按原命令字符串比较路径。2026-11-02 为绕过另一处升级预检的路径表示问题，已获批将同一 Host image 的 SCM 写法改成 `bin/forme-hostd.exe`，当时原 Host 持续运行，没有重启。当前源码 `11dffc1` 已支持等价分隔符，但没有安装到本机。

本轮从 SCM 重读的 Host 命令，对旧版本以反斜杠 image 派生的两个精确参数/引号变体均不匹配；只统一路径分隔符后才匹配。在同一机器用五行无依赖、无 unsafe 的本地观察程序，以混合分隔符启动，实际 `std::env::current_exe()` 返回反斜杠路径。该程序只输出自己的 image 路径，不启动产品、不读安装配置或凭据。这证明当前登记不能满足旧 Host 的启动绑定谓词，必须先消除该阻断；仍不能用推导代替修复后的真实启动结果。

受保护 `startup-failure.json` 虽可读，但修改时间为 **2026-10-27 05:02:32 UTC**，不是本次失败。旧服务在 SCM 绑定和配置装配成功后才保存诊断目录，因此这两个前期阶段失败可能不更新该文件。本轮不能用其中旧 `management_endpoints` 阶段或旧错误摘要解释 11 月 5 日启动失败。

## 2. 本次精确变更

仅将 **forme-host 的 PathName** 从以下值恢复为同文件的 Windows 标准写法：

```text
原："C:\Program Files\ForMe\bin/forme-hostd.exe" --config C:\ProgramData\ForMe\config\host.json
新："C:\Program Files\ForMe\bin\forme-hostd.exe" --config C:\ProgramData\ForMe\config\host.json
```

Web/Brain/updater 的登记、四服务账户/启动方式/依赖/SID/SDDL/有限恢复策略、六程序、六配置、数据和 keys 保持。本次不读取登录密码、系统 bearer 或模型 key，不修改防火墙、端口、TLS 或 Agent 权限。

变更前在 `C:\ProgramData\ForMe\host-state\service-path-restore-20261105\original-services.json` 创建仅 SYSTEM/Administrators 可访问的原登记、Host policy、文件摘要和 journal 水位备份。该目录及文件必须不存在；不覆盖、删除或接管既有内容。新增此独立受保护维护证据不是脑状态变化，也不添加产品 journal 事实。原 2026-11-02 路径修复记录继续保留，后续历史连续性审查必须同时考虑这次反向表示修复。

全量复核后只请求原 Host start 一次，连续三秒保持相同 Running/PID 后才请求原 Web start 一次；Brain/updater 仍停止。旧 Host 会按自身既有启动逻辑打开原 journal，不能在活跃 writer 外另开读取绕过锁，也不以未能重读活跃文件推断其内容。没有 Upgrade、备份/切换命令、历史恢复决定、程序替换或数据迁移。源码读取/SCM 稳定并不替代正式维护投影和原生 IP 验收。

任一步不明停止，不自动回滚登记、反复请求 Start 或重跑旧 Upgrade。Windows 原有限失败策略保留，可能产生额外的 OS 自动启动，必须区分一次显式请求与实际进程次数；本次不为规避恢复策略而改变系统配置。

## 3. 固定材料与执行

- 脚本：`C:\Users\shu\.codex\worktrees\bootstrap-claim-journal\forme\target\legacy-release-review\native-original-host-path-restore-20261105.ps1`。
- SHA-256：`93defb96100806aac062a3298cadee9d098144b563f9ae657e38a7fd2a9d43bf`。
- 隔离检查：Windows PowerShell 5.1/PowerShell 7 各 **29 项 PASS**，仅测试固定四服务状态/命令、备份 ACL 的正反 fixture 与动作 AST；真实服务动作为零。它们不证明原生恢复成功。
- 默认无参数为管理员只读预检；仅在精确批准后以 Windows PowerShell 5.1 运行 `-Apply`。后者使用 .NET Framework 的原子私有目录创建重载，不用 PowerShell 7 执行变更。
- 脚本锁定原只读 guard、原配置/程序基线、上次失败恢复和本次只读诊断的摘要；预检拒绝漂移或已有备份/结果。Apply 结果为另一个 create-new 文件，未结束 marker 不允许重放。

预检尚未取得管理员执行时，脚本又补上私有备份目录的 owner/保护标志/仅 SYSTEM 与 Administrators 两条完整继承规则的显式检查，写入后与结束前重查备份摘要/ACL；没有改变变更范围。上述摘要与 29 项结果指该最终版本，早先 23 项和旧摘要不作为执行依据。

当前只读诊断结果 `native-readonly-failure-analysis-20261105.json` 摘要为 `26c9ff3e9cc22f7369529ba759713cd99410f75db91f4ee1d3d96ce6773e06cf`。观察程序源码摘要 `cbc838db799263a4fe4fea6f8bcf5c34135996734e532b5698c4e4c3617bff74`，本机 executable 摘要 `6bac55da3dcd3bd57e35ef1236e37641141cb9a2a0206f6d8ccdf12d83f2d54c`。这些本地诊断文件均不纳入产品包。

## 4. 批准边界

此前“只读诊断并恢复原 Host/Web”的一次恢复已经实际请求 Host start 并失败，本次还新增了 SCM PathName 修正。依据[原生验收计划 §3](installed-product-windows-native-acceptance-plan.md#3-回退与留存)“重试或卸载另需确认具体操作”，需 owner 对本计划的一次 Apply 作最后确认。既有诊断授权继续涵盖只读预检，不再为同一预检拆分聊天授权；系统 UAC 仍由 Windows 交付管理员令牌。
