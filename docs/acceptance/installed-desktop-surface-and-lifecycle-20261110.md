# 安装版桌面区域与配置生命周期修复验证

本记录承接已批准的桌面授权/截图输出契约。区域映射实现为 `8635b6535a0847b60e03973203af58a5fe75e5f1`，配置生命周期修复为 `71cc476bc4003655c4113cef58be5ac8359d8d93`。这里记录源码验证与签名开发候选；原生截图、完整 IP 或正式发布尚未由本记录验收。

## 两处实际接缝修复

安装 placement 使用 `surface:windows-default`，原生驱动识别具体显示器。现在只在原授权 bounds 完整落入唯一显示器时转换为该显示器的相对坐标，实际 capture 仍只调用一次。跨屏、越界、镜像歧义、重复标识与整数边界均拒绝，不扩大截图范围。支持负桌面坐标。

保存桌面设置会生成 schema 11，但发布升级绑定和恢复数据绑定的两条路径原先将版本强制写成 10，使后续校验拒绝该配置。现在保留已支持的原版本。新增两项行为回归分别验证启用/关闭、升级/恢复、重开后的原 placement、旧 revision 字节不变，以及过期恢复请求不能再次发布。原实现真实复现 0 passed / 2 failed；首次夹具编译失败与后续真正复现的失败原件分开保留。

## 验证结果

| 范围 | 结果 |
| --- | --- |
| 区域映射源码门 | fmt/check/test、strict Clippy、产品构建、installed-host、质量与完整合规 PASS；192 targets，1,454 passed / 0 failed / 22 默认 ignored。installed-host 明确 IP01–IP24 NOT_RUN。 |
| 配置生命周期最终源码门 | 七阶段均 exit 0：fmt、workspace/all-targets check/test、strict Clippy、产品构建、质量、完整合规；192 targets，1,456 passed / 0 failed / 22 默认 ignored。 |
| 独立原件验证 | 最终 1,020 个源码条目及七份日志逐字节核对，测试计数和空间报告一致。 |
| 干净提交 D RuntimeOnly | `71cc476` 的 73/73 场景通过并 seal；独立 verifier 对源码、清单与回执验证 PASS。不是完整 D、C24 live、Final 或 IP 验收。 |
| 代码质量 | 990 files，63 项既存规模债务；未刷新基线。 |
| 构建空间 | 固定 target 逻辑 62,003,766,087 bytes，硬链接去重分配 58,854,480,972 bytes，低于 64 GiB 预算。未清空缓存。 |

最终源码门 UTC 2026-11-09 16:51–17:09；D RuntimeOnly 于 17:13 完成。时间在 Asia/Shanghai 均为 2026-11-10。

## 签名开发候选与原生窗口

候选 `forme-windows-x86_64-20261110-71cc476-dev` 已完成构建、开发签名和正式验证器校验。release digest 为 `sha256:19f2fbae5ec915f66d2aea2baefbaba79cadafdd27d9aebb26695d9a0d22a180`。独立读取核对七个产品二进制、20 个封闭内容文件，以及截图 v2 声明与确切 helper 内容的绑定。候选保持 `release_ready=false`，不属于正式发布。

UTC 17:00 的认证只读预检仍是 `36f8662-dev`：Host/Web/Brain Running，updater Stopped，product/core Ready，revision/ack 3/3，issues 为空。UTC 17:16:42 发起一次原生 UAC 请求，绑定上述候选及固定升级脚本。请求原件本身不是管理员启动或升级完成证明；原操作只核实、不换号重发。原生升级、启用桌面、唯一会话授权、精确自建窗口截图和实际浏览器原件核验将另记结果。

## 原生提权未启动的核对

UTC 17:17:34，原 launcher 返回 `NATIVE_LAUNCH_NOT_CONFIRMED`，进程 ID 为空，异常类型 `InvalidOperationException`。现有记录没有内层 Windows 错误码，不能据此断言用户取消或确定系统原因。没有自动重发提权。

随后认证只读检查确认仍为 `36f8662-dev`、Ready、revision/ack 3/3，三个服务的 PID/路径保持不变，未发现该升级脚本进程。直接查询两个尚未出现的 operation 返回 502，因此不将该响应当成“不存在”证明；改查成功返回的正式维护投影，product revision 71、7 个操作中均无本次 stage/upgrade，且未决操作为空。没有新安装升级回执，未启用桌面或执行截图。用户对 UAC 窗口的观察仍待补充，候选与原启动证据保持。

## 保留的原件

下列文件位于本工作树 `target/product-integration-evidence`；账号凭据与密钥不进入仓库。

| 原件 | SHA-256 |
| --- | --- |
| `desktop-surface-final-gates.json` | `d350759f3aae40ff3ca0faefd4a73b3c33eef6ae51a0603ef35c1ff448f6deb7` |
| `desktop-lifecycle-red2.json` | `2d3a440ffa576c1e985c68c975230dd1db3d3b1fca2389dbd962eb263c645062` |
| `desktop-lifecycle-final-gates.json` | `b0d7c6a4152f0f7e0b945a98c53e8a85d5e0ef104443aaa905ae1f2676c4f7dd` |
| `desktop-lifecycle-final-independent-verification.json` | `efbac7d3b7a45b1b4d35306a8346f991950f5edaea6a31a9e54829c2782380f9` |
| `desktop-lifecycle-clean-d/runtime.json` | `4caac0d43c516c3583d1c27ba2fa4c14a07b57646bf392136188ed4f54523fa4` |
| `desktop-lifecycle-clean-d-independent.json` | `fb4ce2d1d6a8580f3684b24dec54225723722a3ee3231291a4125e0be2f886fd` |
| `desktop-surface-release-candidate.json` | `ad11120c285303ae928dc82c6ed01c8324f1b0f7bf65ff78a6f5015550bc18b9` |
| `desktop-surface-candidate-independent.json` | `c1a5215493dc8e9f57be8f35f97e49ccadec88466920757b113930e946af9c93` |
| `desktop-output-elevation-request.json` | `043d9ab45f4ea0781fc9bdc36cd705b7b2d2cdf084a3c787bfab66cb3f2552b6` |
| `desktop-output-elevation-launch.json` | `003bacf033a33bcabdd8e40003b71b942713cfb2f01a3eb4dc91cb6eefc71c5e` |
| `desktop-output-launch-inspection.json` | `f56a17edb8b887d4dbfa49b8a310841eee76f174cd213ce80e12f393b12ef857` |
| `desktop-output-maintenance-inspection.json` | `d91008c2c625a6cfd9f4fc2d4d90d3ce0048c4fa78a710f6670004a789541c0f` |

## 尚未关闭

Host 执行域副本删除、首次安装可信截图支持、输入引用、PTY 生产接线、受管 MCP/插件/渠道消费者、其他 placement/平台与完整 IP01–IP24 继续开放。组件通过、签名或 UAC 请求均不能替代原生联合证据。
