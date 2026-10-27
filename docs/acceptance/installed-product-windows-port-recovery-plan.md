# Windows 既有开发安装的端口恢复计划

2026-10-27。**四配置十值修复完成；另获批准的一次 Host/Web 启动序列已完成，当前两个服务均 Running，真实浏览器可见 `http://127.0.0.1:18765` 初始化页面。Brain/updater 仍停止，安装仍未认领。** 原版/V2 和启动观察脚本的失败证据原样保留，详见 §5–8。本计划不是 IP 通过报告。承接[原生验收计划 §3](installed-product-windows-native-acceptance-plan.md#3-回退与留存)和[真实安装记录](installed-product-windows-native-install-20261027.md)。已消费的一次性批准不复用。

## 1. 原因与最小范围

本机 Windows 排除范围 `8730–8829` 覆盖原 Web `8765` 和内部 `8766–8769`。独立 TCP bind 对 `8765` 返回 `10013`，`18765–18769` 均可绑定。此前新安装端口修复只覆盖内部端口选择，不会修改已经安装的配置，也未替 owner 选择新的公共地址。

此次只处理尚未认领、无脑数据的开发安装。拟将网页地址显式选为 `http://127.0.0.1:18765`，内部端口使用 `18766–18769`。不启用 LAN/TLS、不修改防火墙；当前 loopback HTTP 的精确 Host/Origin 校验继续从实际绑定派生，不加入通配或额外允许源。

| 受保护配置 | 唯一允许改变的值 |
|---|---|
| `C:\ProgramData\ForMe\config\host.json` | `host_bind`、`agent_execution_bind`、`configuration_bind` 分别 `8766/8767/8768 → 18766/18767/18768`。 |
| `C:\ProgramData\ForMe\config\product-template.json` | `core_bind` 为 `8769 → 18769`；`web_bind` 为 `8765 → 18765`。 |
| `C:\ProgramData\ForMe\config\web.json` | `bind` 为 `8765 → 18765`；bootstrap/configuration/core 三桥地址分别 `8766/8768/8769 → 18766/18768/18769`。 |
| `C:\ProgramData\ForMe\config\web-network\web-network.json` | loopback overlay 的 `bind` 为 `8765 → 18765`；TLS/firewall/allowed_hosts 保持原样。 |

共四文件、十个值，IP 地址均保持 `127.0.0.1`。转换保留字段外全部原始 UTF-8 字节、空白、键序和 ACL，不整体重新序列化 JSON。

六个已安装程序（包括上次已获批替换的诊断 Host）、release manifest/identity、密钥、服务注册/Service SID/恢复策略、已有 journal 和本仓库 `.forme` 不在修改范围。此安装仍为 `7cb5b8b` 的开发诊断实例，不伪装成新候选或签名发布。

## 2. 固定可审查脚本

- 主脚本：`C:\code\forme\target\installed-file-worker-evidence\native-internal-port-recovery.ps1`，SHA-256 `ff109427b1fd377d2dbf935654041cbc1f9bd8a9af198eb389698e042c5a2c2f`。
- 转换/原子替换函数：同目录 `native-port-recovery-functions.ps1`，SHA-256 `329c3504fb7212a131b1ce0a6be17e3e34771da2bd86759953515b90adcaf96d`，由主脚本固定核对。
- 默认不带 `-Apply` 只读；新执行必须明确批准**一次** `-Apply`。需要 UAC，脚本不自我提权/重试。

前置条件：管理员、固定安装根与无 reparse、四服务均停止且绑定与原基线一致、六程序摘要准确、原配置/密钥摘要一致、brain/default workspace 为空；不存在此次 attempt/report/backup/staging。全部安装目录只读快照包含内容摘要和 SDDL；再实际绑定五个新端口，任何不明即拒绝。

Apply 顺序：创建不可覆盖 attempt → 在管理员/SYSTEM 私有目录备份四原件并核对摘要 → 为每文件创建相邻私有 staging、保留 ACL、刷盘、重查原摘要后原子替换 → 以安装全域快照证明仅四配置摘要改变 → 释放端口探针 → 请求 Host 启动一次并连续观察 Running/PID 至少三秒 → 才请求 Web 启动一次 → 重查保护原件及服务绑定。

Brain/updater 不启动；不认领账号、不写模型配置、不重跑安装器。Host/Web 的正常启动可写各自产品/认证运行状态，这与配置替换分开记录，不能冒充大脑已 Ready。

## 3. 失败与原件

备份固定在 `target/installed-file-worker-evidence/original-config-before-internal-port-recovery/`，目录仅 Administrators/SYSTEM 可读。attempt 为 `native-internal-port-recovery.started`，最终脱敏报告为 `native-internal-port-recovery.json`；均禁止覆盖和自动重复执行。

四文件逐个原子替换，**不宣称跨文件原子事务**。任一步失败保留已变更文件、未完成 staging、原件和报告；未全部验证前零 Start。启动失败/超时后不自动回滚或再次请求 Start，不修改现有 SCM 有限恢复策略。恢复/撤销本次变更需要另行审查具体现场。

## 4. 已完成的准备证据

- 自建临时夹具：PowerShell 5.1 与 7 各 **23 项断言 PASS**，验证仅改白名单值、其他字节保持、错误/重复字段拒绝、陈旧摘要零写、ACL 保持、已消费替换拒绝、部分失败保留、已有 staging 拒绝。夹具临时目录已清理；未访问安装数据。
- 管理员只读预检：`native-internal-port-recovery-preflight-5.txt` 返回 `preflight_passed_no_change`、`read_only=true`、安装快照 **66 项**、`protected_files_unchanged=true`、`start_requests=[]`，退出 `0`；四服务仍停止。
- 早期预检失败日志 `-1` 至 `-3` 保留，分别暴露脚本 inventory 路径与公共 `8765` 绑定问题；没有执行配置替换或 Start。`-4` 首次通过端口/原件预检，`-5` 进一步包含完整快照。

通过这里只表示已准备好限定恢复。网页可达、认领/模型/真实 worker、服务重启、完整 IP01–IP24、正式签名和平台支持继续分别验收；Linux/macOS 维持既定不发布边界。

## 5. 原版 Apply 的实际结果

owner 明确批准后执行一次固定脚本，退出 `1`。`native-internal-port-recovery.json` 为 `failed_preserved`、`failure_stage=replace_config`、`changed_files=[]`、`start_requests=[]`、`backup_verified=true`。失败位于函数第 42 行：空 staging 的 ACL 与原配置不同，替换前停止。四服务均为 Stopped、PID 0。

后续管理员只读 `native-port-recovery-acl-diagnosis.json` 独立核对四原配置、全部既有数据/密钥、六程序及四备份摘要均匹配；brain/default workspace 仍为空。仅新增 `config/host.json.internal-port-recovery.tmp`，长度为 0；该文件、原报告、attempt 和备份保留，不删除或改写。原报告的 `protected_files_unchanged=false` 是 Apply 尚未成功赋值的字段，不代表已证明原件改变；独立只读核对为原件保持。

根因是 `Get-Acl` 返回的安全描述符没有修改标记；直接传给 `SetAccessControl` 不会将原受保护 DACL 写入新文件，新文件仍保留父目录继承规则。此前普通临时文件夹具的源/目标同为继承 ACL，漏掉了此场景。不能删除权限断言或放宽安装 ACL 来绕过。

## 6. V2 恢复与实际结果

V2 范围仍为 §1 的四文件十值、一次 Host 启动及 Host 稳定后的一次 Web 启动；不启动 Brain/updater。唯一修正为从原 SDDL 构造新的 `FileSecurity`，明确写入 owner/group/DACL 后再验证完全相等。旧空 staging 原样保留，使用新的 `.internal-port-recovery-v2.tmp`；旧失败报告、attempt、备份和只读诊断均先固定摘要验证。

- 主脚本：`target/installed-file-worker-evidence/native-internal-port-recovery-v2.ps1`，SHA-256 `19a5badedacd00562e67792712fdc70476bfa338f1a589d25d6c0db59a46d5a0`。
- 函数：同目录 `native-port-recovery-functions-v2.ps1`，SHA-256 `4489f1f8e8889d33153eecd6da0b9bd47065834e2b389dedca10a494dac0aaf9`，固定引用原版函数摘要，不改原脚本。
- V2 独立 attempt/report/backup 分别为 `native-internal-port-recovery-v2.started`、`native-internal-port-recovery-v2.json`、`original-config-before-internal-port-recovery-v2/`；均禁止覆盖、自动回滚或重试。
- 管理员自建保护 ACL 夹具：PowerShell 5.1/7 各 **30 项断言 PASS**，分别覆盖仅管理员/SYSTEM 和额外 Web Service SID Read 的配置，复现原版失败、核对原件保持，验证 V2 精确 ACL/内容替换、旧 staging 留存、陈旧摘要/ACL 和重复执行拒绝。临时夹具已清理，不读取或修改安装数据。
- V2 管理员默认只读预检：`native-internal-port-recovery-v2-preflight-1.txt` 退出 `0`，`preflight_passed_no_change`，全域 **67 项**快照（包含原空 staging），原保护文件保持、旧 attempt/staging 保持，`changed_files=[]`、`start_requests=[]`，五端口可绑定，四服务全停止。

owner 随后明确批准并执行一次 V2 Apply。四文件替换及启动前全域复核通过，十个端口值均已改变，ACL 保持；在 `start_host_once` 退出 `1`，未尝试 Web。报告 `native-internal-port-recovery-v2.json` 保留全部四组 before/after 摘要、旧现场保持和唯一 Host 启动尝试。

独立管理员只读 `native-port-recovery-v2-readonly.json` 为 `new_ports_verified_services_stopped`：四新配置摘要及原 ACL 相符、其他数据/密钥和六程序未变、两套四原件备份均相符、旧空 staging 保持。四服务仍 Stopped。原 Host `management_endpoints` 诊断文件修改时间为 `05:02:32 UTC`，早于 V2 `08:30:32 UTC`；不能把旧诊断当成 V2 的新 Host 故障。

启动辅助进程的只读复现发现：Windows PowerShell 5.1 新 Job 未加载 `System.ServiceProcess`，构造 `ServiceController` 报 `TypeNotFound`；PowerShell 7 可构造。V2 的 `start_requests` 是在发起 Job 前记录的尝试，不能证明 SCM 接受了 Start。原 Job 已按原脚本结束清理，真实 SCM 请求状态没有独立回执；此处如实保留证据限制，不重写原报告。

## 7. 已批准的纯启动修正

端口配置已经完成，下一操作不再写配置、ACL、程序或密钥。固定脚本 `target/installed-file-worker-evidence/native-post-port-start.ps1 -StartServices` 只请求 Host Start 一次；连续 Running/PID 三秒后才请求 Web 一次，并要求最终二者均 Running。Brain/updater 保持停止；失败不重试、回滚或重装。

- 主脚本 SHA-256：`433c9ba3453a8962565b9c19e8771d94cac593d0eb175eff9bbb0a8bc95c1453`。
- 有界 Job helper `native-service-request-job.ps1` SHA-256：`a6873fff3fd774b28a8201e3ea7a50e1581070c56341fbecd903fbe92a2baa9b`。子进程显式加载程序集；Start 前后/失败均返回结构化 receipt，父进程在移除 Job 前保存该结果，不输出原始错误文本。
- helper 在 PowerShell 5.1/7 的只读观察各 **9 项断言 PASS**，包括精确服务 allowlist、实际类型加载/控制器读取及零 Start 调用；这不是实际启动通过证明。
- 管理员无 `-StartServices` 预检 `native-post-port-start-preflight-1.txt` 退出 `0`，`preflight_passed_no_start`；四新配置/原 ACL/旧报告/备份/keys/程序及服务绑定均核对，五端口可绑定，零配置/ACL/程序写入、零启动尝试。
- 独立新 attempt/report 为 `native-post-port-start.started` 与 `native-post-port-start.json`；均 CreateNew，不覆盖 V1/V2 的日志和证据。

owner 明确批准后执行一次纯启动。Host 的结构化结果为 `start_invocations=1`、`request_returned=true`、`status=StartPending`、无异常。观察脚本把 Job 完成时的一次旧 Stopped 样本误判为失败并提前结束，故原报告为 `failed_preserved`；但该报告最终 SCM 状态已经是 Host Running、PID `55500`、退出码 `0`。后续独立只读连续观察确认同一 PID 稳定，内部管理端口 `18766/18768` 由该 PID 监听。不能把这个观察器失败报告改写为成功；也不重复启动 Host。

## 8. 同一授权序列的 Web 首次启动与网页实测

上述批准包含 Host 稳定后的 Web 一次启动，Web 尚未被调用，因此继续完成该未消费动作，无新增重启/配置授权。限定脚本 `target/installed-file-worker-evidence/native-web-first-start.ps1` SHA-256 为 `50eec67138a9c74dd8b73e7c32c915240953971932238b70783868064c7231c5`：核对当前 Host 固定 PID/绑定和所有保护输入，稳定观察后只请求 Web 一次，不含任何 Host Start/Stop。其观察窗口允许初始旧状态，不用 Job 完成与旧 Stopped 的竞态推断终态。

第一次 wrapper 仅运行默认只读预检，因未初始化的 PowerShell `$LASTEXITCODE` 提前结束，未创建 Web attempt、未调用 Start；该日志保留为 `native-web-first-start-preflight-1.txt`。修正 wrapper 的本地返回码初始化后再次只读预检通过，实际第一次 Web Start 才执行，报告 `native-web-first-start.json` 为 `host_web_running_unclaimed_development_only`、退出 `0`。记录 Web `Stopped → Start Pending → Running`、`start_invocations=1`、`request_returned=true`，最终 PID `49552`、退出码 `0`；`host_start_calls=0`。没有服务重试、配置/ACL/程序写入或安装器重跑。

独立 SCM/端口及浏览器结果：

| 检查 | 当前证据 |
|---|---|
| Host | Running、PID `55500`、LocalSystem；`127.0.0.1:18766/18768` 监听。 |
| Web | Running、PID `49552`、`NT SERVICE\forme-web`；网页 `18765` 可达。 |
| Brain/updater | 均 Stopped、PID `0`；未从恢复脚本启动。 |
| HTTP/API | `/` 返回 HTML 200；`/api/v1/bootstrap/status` 为 `claimed=false`、`setup_needed=true`。 |
| 真实浏览器 | 实际打开初始化页面，显示本机一次性证明输入；资源/交互界面已渲染。未提交表单或输入凭证。 |

无凭证截图保存在本地 `target/installed-file-worker-evidence/native-initialization-18765.png`。此处只关闭“旧安装端口修复后 Host/Web 能启动且初始化可见”这一诊断问题；认领、真实 provider/worker、注销/冷启动、升级/卸载、签名和完整 IP01–IP24 继续分别验收。
