# Windows 登录会话 helper 的原生实现边界草案

状态：待 owner 批准；本草案不激活 backend，不扩展当前 unsafe 白名单。它承接 PRD24 §4.11、architecture/10 §7.2、canonical §29.10 和 IP11，不增加功能内核、crate、依赖边或脑 EventKind。

## 1. 需要批准的具体变更

当前 `DesktopHelperAdapter` 已有 exact grant/plan/capability、终态幂等与 Unknown 合同，但生产仍为 `UnavailableDesktopHelper`。现有三个获准 Win32 adapter 仅覆盖 AppContainer 与 Brain↔Host AgentExecution，不能承载登录用户 token、交互会话启动和 helper peer 身份。

建议只新增以下两个经过审计的 Win32 边界文件，并在批准后更新 AGENTS 硬规则 5、canonical §29.9、PRD24、architecture/10、质量门及许可证/依赖记录：

| 文件 | 唯一职责与允许的原生操作 |
|---|---|
| `crates/execution/src/installed/windows_desktop_session_native.rs` | 查询实际 WTS session、登录用户 SID、token 的 AuthenticationId/logon generation、连接与锁屏状态；仅 LocalSystem Host 可取得目标用户 token 并以该身份启动固定活动发布的 helper；有界进程/job 生命周期与句柄回收。 |
| `crates/execution/src/installed/windows_desktop_helper_peer_native.rs` | 安装和单次 helper 绑定的私有 named pipe；只允许预期 Host 与登录用户，核验两端实际 PID、token user/session/logon generation 和活动发布的确切 image；有界 IPC 与 deadline，默认拒绝错误 peer。 |

其他编排、journal、binding、参数/receipt 检查和 helper 执行逻辑保持 safe Rust；每个新增生产文件遵守 600 行限制。不能把新原生职责放进现有三个文件来规避审核。只扩展已 pin 的 `windows-sys` feature 集，不默认增加第三方 crate；若最终需要额外依赖，先补来源审查。

## 2. 固定程序与执行链

复用第六个产品程序 `forme-executord` 的固定 `--desktop-session-helper` 入口，不增加常驻大脑或独立模型配置。Host 从与文件 worker 相同的已验证活动发布派生 image；helper 不接受任意程序路径、环境或外部 helper 地址。桌面动作复用已有 `NativeComputerDriver` 的公开安全接口和六类 `ComputerOperation`，不再手写另一套输入或截图 FFI。

链路保持 `owner 认证/授权 -> ProductionRuntime/Harness -> 最终 plan/permit -> WorkerJob -> Host -> helper`。安装提权不授予 Agent 桌面权限；浏览器提交的 OS user、session、PID 或地址均不是身份依据。选择会话必须引用 Host 实际枚举且在执行前重新核验的候选，并同时具备现有桌面 grant。

Host 与 helper 都检查具体登录用户、session、AuthenticationId、默认交互桌面、锁屏与 TTL；用户名、PID、wall-clock 或 session number 不能独自代表登录世代。helper 不借 SYSTEM 执行输入，不进入 Winlogon/UAC secure desktop。缺少用户、会话不可验证、锁屏、注销或 peer 变化均阻断新动作，非交互服务保持运行。

交互 PTY 仍必须绑定既有已准入的 PTY 会话及其 exact action；本草案不允许把 `PtyInput` 解释为创建任意 shell。未能满足该绑定的能力继续按真实状态不可用，不能用 Computer 实现替代 PTY 验收。

## 3. 数据、一次性与恢复

helper 没有 brain DB、keys、EventStore、模型、policy/config mutation 或安装管理能力。IPC 不使用浏览器 bearer；会话/动作 token 不进入 Web、URL、日志、脑事件或诊断包。截图和输出受原 plan 的大小、时间、scope 与结果封存预算约束，原 helper receipt 经过 Host 核验并进入既有加密结果通道。

Host 的产品侧 session/worker journal 单写者先记录原 action 的 intent，再派发一次。开始后丢失 helper/Host/可信终态均为原 job 的 Unknown；启动恢复只 probe/核实原 receipt，不重派。取消先终止原 helper 树，无法证明停止时保留 Unknown；锁屏或 TTL 到期不推定已启动动作成功或已取消。

升级、恢复和卸载先撤销 grant、关闭新 admission、排空 helper，再核对原 receipt。旧 generation、nonce 或授权不得因重启/重新登录复活，Unknown 证据不得被清理。

## 4. 需要交付的证据

| 层级 | 必须证明的内容 |
|---|---|
| 合同与负向测试 | 错 owner/session/generation、陈旧 grant、plan/nonce/capability 替换、错误 peer、过期、重复命令均无桌面调用；已经开始后断线保留 Unknown。 |
| 当前用户原生组件 | 在独立临时窗口内验证输入、截图、范围、结果封存、超时/取消；不操作用户其他窗口或真实数据，不折算 SCM。 |
| 真实安装/登录会话 | LocalSystem Host 以实际用户身份启动 helper；锁屏、注销、重登录、无用户、假 peer、重启恢复、升级/卸载排空；证明不能读服务私有 brain/keys。 |
| 完整回归 | 当前源码质量门、Host 永久门、S1–S99、适用 C 门与当前源码审计；组件 PASS 不能替代 IP11/IP24。 |

原生安装、管理员/UAC、系统账户/服务注册和用户桌面实际输入另需具体操作授权。Linux/macOS 不因本草案扩大支持声明，继续保持 owner 已选择的有限恢复与 `release_ready=false`。

## 5. 批准的含义

批准本草案意味着允许先回填上述权威文档和显式 unsafe 白名单，再实施这两个边界及 safe Rust 装配/测试。它不等于准许安装本机系统服务、操作其他用户桌面、merge、push 或发布，也不等于任何原生产品场景已经通过。
