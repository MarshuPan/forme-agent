# 安装版文件执行与 AgentExecution 管道验收（2026-11-06）

本批继续同一安装验收目标，承接[本机重置记录](installed-product-local-reset-20261106.md)、PRD24 §4.1.1b–e 与 IP10/IP18/IP24。管道修复实现为 `e0571418194052c231afda78d7d4221d6a020879`。本报告区分源码门、真实 AppContainer 组件和已安装服务；当前文件全链路仍未通过，整体目标未完成。

## 1. 首次真实执行发现的问题

正式 Web → Gateway → ProductionRuntime 的真实模型提出 `tool:file`。拒绝写入场景通过：审批前后磁盘均没有目标，`ActionStarted=0`，最终 Aborted，同一决定重放零新增动作。批准写入的参数摘要替换被 HTTP 400 拒绝，动作仍未开始；随后正常批准产生一次 ActionStarted，却进入 ActionOutcomeUnknown。

首次批准场景的原 run 为 `run:1793947733947220800:5`，原 job 为 `worker-job:sha256:4580f876c3c9861cdbe881680e47279a6896855e13da303936c2a2175782598d`。Host 在接收 job 前退出。其 serving 错误摘要与 `AgentExecution named pipe accept failed` 的既有摘要域完全匹配；独占停读窗口核对 worker journal 为零字节、空文件 SHA-256，sandbox lease 也为空，磁盘目标不存在。

根因是每次创建后续 listener 都使用 `FILE_FLAG_FIRST_PIPE_INSTANCE`，已有接受连接仍持句柄时必然冲突；提前断开的客户端还会触发 `ERROR_NO_DATA`，旧实现也把它作为服务故障。

修复在 bind 时一次性认领首实例；接受或拒绝当前连接前建立非 first 的后续实例，持续持有命名空间。提前断开和身份不符的 peer 被拒绝，不退出 Host；客户端仍先验证 SCM PID、账户和实际 image，之后才发送凭据。原 Service SID、安装 image、bearer 和最终 plan 治理保持，未增加 unsafe adapter、DTO、trait、crate、依赖边或 EventKind。

## 2. 已执行的验证

| 证据层 | 本轮结果 |
|---|---|
| 定向管道测试 | 6/6；含并发 in-flight 实例、持续命名空间持有、提前断开的非可信 peer、伪造 Host 零凭据发送及有界读写。测试移到独立 `forbid(unsafe_code)` 文件，两项新增场景纳入永久宿主门。 |
| 完整源码质量门 | exit 0：182 Rust targets，1295 passed / 0 failed / 15 默认 ignored；fmt、locked workspace check/build/test、strict Clippy、前端 115/115、依赖审计、嵌入资源、合规与结构棘轮通过。规模为 874 文件、63 处既有规模债务，未刷新基线。 |
| 原生安装器退出夹具 | 2/2；仅临时成功/失败退出，不安装 ForMe 或改变服务。 |
| 永久宿主门 | exit 0，381/381 逐名场景通过；其输出仍明确 IP01–IP24 NOT_RUN，不能代替安装服务验收。 |
| 真实 AppContainer 文件组件 | 显式运行原先默认 ignored 的 `native_installed_file_entry_uses_exact_mapping_host_writeback_and_encrypted_results`，1/1，分别执行 Read/Write/Delete，核对 Host 回写与加密封存；未使用已安装 SCM 身份，不能升级为 IP10 通过。 |
| release 与签名候选 | 完整 `cargo build --release --locked --workspace` 成功。干净 `e057141` 又生成完整 bundle、独立 Ed25519 签名发布和开发签名安装器，并由当前 Rust 离线 verifier 核对。仅本机开发签名，`release_ready=false`。 |

完整源码门首次因 MinGW 链接 OS1450 资源不足停止，保留失败日志；将 Cargo jobs 降为 2 后，同一完整检查集合通过。单包 release 构建命中旧协议缓存的失败日志也保留，未据此修改协议；标准完整 workspace release 构建通过。随后完整永久 M5 门运行至 S99 发布审计：场景、golden、workspace 测试、Python 95 项（1 项 skipped）、合规及历史 M4 回执独立核验通过；当前发布审计因本报告和进度表尚未提交而报 `RELEASE_TREE_DIRTY`，exit 1，故本轮不记为 `M5 FINAL ACCEPTANCE: PASS`。保留原日志，提交证据后只补跑发布审计及其独立核验/比较，不重复已通过的整轮检查。

## 3. 原 Unknown 的显式处理与本机修复

在核实上述原件后，已通过正式 owner 控制入口对原 action 显式作 Cancelled 裁决。原 run 最终 Aborted，保留唯一 ActionOutcomeUnknown，再追加 ActionCancelled/RunAborted；没有 ActionCompleted。同一裁决重放返回 204，新增 ActionStarted 为零，目标文件仍不存在。该结果不宣称原动作成功，也没有重派原 job。

本机仅替换固定安装中的 Host image，保留原程序备份，独立复核原配置摘要、安装身份与 ACL。新 Host SHA-256 为 `36dc89c2e33cda013ec698bd533564fd6c8d7d3bb53ea98153becaab4169f84e`。Host/Web/Brain 恢复为 Running，配置保持；这只是单 Host 诊断修复，active release 仍为原 `forme-windows-x86_64-20261106-d22b6a3`，不能称为完整新版本升级。

部署后的包装脚本在成功部署后读取未设置的 `$LASTEXITCODE` 而停止，尚未发出新文件请求。失败回执保留；后续独立脚本只做验证，不重复部署或重启。正式新写入 run `run:1793950668761966500:1` 再次进入 Waiting/ActionOutcomeUnknown，目标不存在；本次 Host/Web/Brain PID 均未变化，首次 Host 退出故障已消除，但文件链路尚有另一处实际失败。当前不重发这个 job、不继续 Read/Delete，也不把临时组件成功解释为安装版通过。后续只读观察核对真实 Brain peer 身份与 worker 准入水位，独立登记原号。

## 4. 浏览器观察与仍开放的义务

真实已登录 IAB 设置页显示主脑已连接、配置 revision 1 已确认；同页 doctor 为 blocking，明确列出 notification 缺 worker binding，shell/browser/computer/PTY disabled，以及 MCP/App API/plugin 未配置。受管设置中 MCP、plugins、packages、channels 仍为 `managed_consumer_missing`；这些是实现/接线义务，不把“状态展示诚实”算成功能实现完成。

本批仍需完成当前文件 job 的原号核实、服务身份/transport 的剩余修复、真实审批读写删与隔离反例、正常完整签名升级/恢复及余下 IP。桌面 helper、OS 无登录冷启动/注销、LAN/TLS、其他平台和正式签名发布分别保留原生证据边界；没有执行的项目不计通过。

本地工件固定在本工作树 `target/native-legacy-review-20261106/`，不含测试登录密码、cookie、bootstrap proof 或 provider key。新开发候选为 `forme-windows-x86_64-20261106-e057141`，其正常升级尚未执行。

| 已结束工件 | SHA-256 |
|---|---|
| `worker-pipe-source-gate-jobs2.log` | `a70cbf62c74bea5a6b4018d50ecc1093fd63238e49b49c5915777c19c53b3eac` |
| `original-file-job-offline-inspection.json` | `40e9ad805782e7b3cec81b375c6cc63ac85bf9cbcf85656af5162c5bc23cd259` |
| `original-file-unknown-resolution.json` | `e9bf06c029c99df7e45f1a7df28dbfde1eff8b056b78f2c51013831fc62b8604` |
| `installed-settings-browser.jpg` | `68d624e753bc4f1d3cea5d4bc3abb955e2f126b76ec8297cbe3a5d719efb5ab0` |
| `pipe-repair-candidate/current-build-observation.json` | `5b54f07605033c6b37004d10f7a80bedc75c9153c1cecd1038e6ea7bbfb98b58` |
| 开发签名安装器 | `5e329cc63a76a3c761b00e080e2871c423a2b75646f5c6e2cd8b57036666b59d` |

后续原生结果继续追加，既有失败不覆盖；文档更新不触发无理由的整仓重复回归。
