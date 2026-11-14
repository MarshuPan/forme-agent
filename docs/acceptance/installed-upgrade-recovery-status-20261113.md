# 当前升级恢复与测试登录（2026-11-13，11-14 更正）

本记录接续 11 月 13 日的签名暂存和升级尝试。当前操作是 `maintenance:upgrade-current-20261113-fa24bee`，不是 11 月 2 日旧安装的升级。owner 已批准一次有界恢复：修正 updater 策略、停止 Brain、执行 `AbortBeforeSwitch` 并保留原版本。该批准不替代恢复接口要求的源证据。11 月 14 日管理员只读检查确认策略原本正确，撤回先前“策略不匹配”的判断；不能再运行旧修正脚本。后续当前结论见[超时与恢复核查](installed-release-timeout-20261114.md)。

## 已验证状态

- 使用原有测试凭据在 Ego Lite 成功登录本机工作台。按 owner 指示，在仓库外保存 Windows DPAPI CurrentUser 加密的 `PSCredential`，目录仅当前用户及 SYSTEM 可访问；重新解密比对通过。密码、会话和 CSRF 不进入本记录或 Git。
- 正式认证 API 返回 product revision **104**，原升级为 **Validated / pending Quiescing**；配置与确认版本均为 **3**，active release 仍为 `forme-windows-x86_64-20261109-36f8662-dev`。
- 恢复操作列表为空；针对原升级的只读 candidate 请求返回 **502 / ConfigurationRejected**。未取得有效 candidate，未提交恢复 POST。
- SCM 显示 Host/Web/Brain 仍 Running，updater Stopped；PID 分别为 5540、23100、23384、0。
- **已更正**：11 月 13 日把 `sc.exe qfailure` 不显示 `SC_ACTION_NONE` 误判为空 actions。11 月 14 日直接核对注册表 `FailureActions`：28 bytes、action count=1、action=None、delay=0，reset=INFINITE、non-crash=false，符合源码。没有策略登记不匹配证据。

## 本次管理员动作结果

11 月 13 日准备了仅校验固定 updater 身份、只读核实 journal 和修正该服务恢复策略的有界脚本；它不启动 updater、不修改发布文件、不停 Brain、不写产品 journal。通过 `Start-Process -Verb RunAs` 请求运行时，Windows 返回“操作已被用户取消”。脚本未产生执行报告；随后 SCM 观察的状态和 PID 未变。**策略修正 NOT_RUN，停止 Brain NOT_RUN，恢复裁决 NOT_RUN。** 当天没有再次发起提权。11 月 14 日另一个纯只读管理员检查已成功执行，原 journal 被活动 Host 的独占锁保护，未绕过锁读取。

## 普通恢复的额外前置条件

PRD24 §4.10.5–6 要求升级前持久化 `ProductReleaseRecoveryBaseline`。`host_release_takeover.rs` 仅在接纳升级前 Brain 已停止且 worker 静止时捕获该基线；`journal/release_recovery.rs` 在读取候选时先要求基线存在。停止正在运行的 Brain 不能补出历史基线。

今天升级前 Brain 运行，暂存后的 revision 99 到原升级的 revision 104 仅增长五条；结合对应源码，这支持“本次没有保存源基线”的判断。`36f8662..HEAD` 的上述 takeover、恢复投影和 updater registration 三文件没有差异。但本轮没有独占读取原 journal，**基线缺失仍标记为源码/回执推断，不冒充原件检查结果**。

后续先核实原 journal 的基线是否存在；若不存在，普通 `AbortBeforeSwitch` 不适用，应按既有缺基线审查契约准备独立证据与精确恢复方案，不能补造基线、猜测 candidate、删除 journal/claim，或重发原 Upgrade。本轮保持 Brain 原运行状态，避免在尚无有效裁决路径时额外停机。

## 本地证据

- `target/product-integration-evidence/abort-before-switch-preflight-20261113.json`：当前认证读取。
- `target/product-integration-evidence/upgrade-release-20261113.json`：原升级 POST 502 和原号查询；不是未接纳证明。
- `target/product-integration-evidence/recovery-authorized-native-20261113.ps1`：基于错误策略判断准备、未运行的历史脚本；已失效，不得执行。
- `target/product-integration-evidence/recovery-launch-cancelled-20261113.json`：管理员启动取消和最终服务状态。

11 月 13 日未修改生产代码、构建或部署，不新增 IP14/IP24 或全产品 PASS。11 月 14 日的独立源码修复与验证另记，不改写当天结果。
