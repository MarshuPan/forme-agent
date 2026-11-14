# 安装升级响应超时修复与现场边界（2026-11-14）

## 结论

当前现场的 updater 恢复策略正确。前一天把 `sc.exe qfailure` 未显示的 `SC_ACTION_NONE` 误读为空 actions，已由直接读取 `FailureActions` 更正：28 bytes，action count=1，唯一 action 为 `None / 0`，delay=0，reset=INFINITE，non-crash flag=false，与 `registration/updater.rs` 一致。不能运行旧的策略修正脚本。

这次升级卡住的一个确定工程缺陷是 Web→Host 的响应等待不匹配：配置桥接对所有请求只读 10 秒，浏览器默认 28 秒，而升级/回滚会在返回 receipt 前完成签名 release 预检、状态投影和 handoff 持久化。原 Host 的顺序是先持久化原 operation/takeover/Quiescing，再写响应，响应成功关闭后才 dispatch；响应失败则不产生 native dispatch。现场 11 月 13 日的 handoff 时间比请求 deadline 推算的提交时间晚约 76 秒，支持该解释，但原请求没有阶段级失败 trace，不能把它宣称为唯一根因。

## 源码修复

- `LocalConfigurationClient` 只对 typed `Upgrade` 与 `RollbackCompatibleRelease` 使用响应预算：当前请求 deadline 的剩余时间（最多 300 秒）加 10 秒收尾余量；普通读取、配置和已过期重放仍为 10 秒。
- Web maintenance POST 脱离普通 30 秒路由层，但仍受 350 秒有界路由、360 秒连接上限和 Host 自身最多 300 秒授权窗口约束。
- 浏览器同样只延长等待预算，不改 request body、operation id、授权 deadline、Idempotency-Key，不重试；升级/回滚最多等待 360 秒，普通请求保持 28 秒。
- 断线仍返回 `ConnectionLost`/未知结果语义，只允许用原 operation 查询；不根据超时猜测未执行，不重新生成 key。

## 验证

- `cargo test -p forme-gateway --lib`：**143 passed / 0 failed / 1 ignored**，96.73 秒。包含 31 秒延迟 Host receipt、断开连接未知结果、错误口令不触达 Host 的真实 loopback HTTP 场景。
- 最终定向复验 `web::http::tests::maintenance_transport`：**2 passed / 0 failed**，35.24 秒。
- `cargo clippy -p forme-gateway --all-targets -- -D warnings`：**PASS**。
- `cmd /c npm.cmd run verify`（frontend）：**PASS**，141 tests passed，静态 Web 资源 7/7 verified。
- `py -3 tools/code-quality.py`：**PASS**，1150 files，62 existing size debts。
- `bash tools/compliance-doctor.sh`：**PASS**。
- `py -3 tools/build_resources.py check`：**PASS**，最后一次检查约 111.8 GiB 可用空间。
- `py -3 tools/build_resources.py report --all-worktrees --json`：本工作树 target 109.7 GB logical / 85.8 GB allocated，超过 64 GiB 预算；`clean` 仅做 dry-run，预览 102.2 GiB、删除 0 files。

以上是源码和组件证据。当前安装仍运行 `forme-windows-x86_64-20261109-36f8662-dev`，没有部署本修复。

## 现场恢复边界

当前原操作 `maintenance:upgrade-current-20261113-fa24bee` 仍为 revision 104、`Validated / pending Quiescing`，active release 仍为旧版；Host/Web/Brain 仍运行，updater 本次开机未启动（SCM 1077），没有本次 operation 的 dispatch claim。原 journal 仍由活动 Host 独占，未绕过锁读取。

Ego Lite 于 2026-11-14 15:58:45（Asia/Shanghai）再次认证只读：product state=`recovery_required`、core health=`ready`、active release=`forme-windows-x86_64-20261109-36f8662-dev`，revision=104；恢复列表为空，原 operation candidate 仍返回 502 `ConfigurationRejected`。没有新的升级请求或服务动作。

普通 `AbortBeforeSwitch` 仍要求升级前可信源基线；这次升级是在 Brain 运行时接纳，当前没有得到该 baseline，停止 Brain 也不能事后补造。候选接口继续返回 `502 ConfigurationRejected`，没有提交恢复 POST。若要处理这次无基线、未确认进入原生执行的操作，需要按已批准的独立历史审查/迁移方案，逐项取得原 journal、源数据、程序、服务、配置和 worker 连续性证据；缺一项保持 `Unverifiable`，不停止服务、不删除 handoff/claim、不重发 Upgrade。

现场证据：

- `target/product-integration-evidence/recovery-native-readonly-20261114.json`
- `target/product-integration-evidence/recovery-preflight-20261114.json`
- `target/product-integration-evidence/recovery-preflight-20261114-followup.json`
- `target/product-integration-evidence/upgrade-release-20261113.json`
- `target/product-integration-evidence/release-timeout-gateway-tests-20261114.log`
- `target/product-integration-evidence/release-timeout-frontend-20261114.log`
