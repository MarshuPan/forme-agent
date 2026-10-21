# 受管服务与 Web 工作台组件记录

日期：2026-10-21。承接 prd/24–26 与代码维护规范。本记录不重开 M6、不改写核心历史验收，不以组件 PASS 替代原生安装或 IP01–IP24。

## 1. 已实现的纵向路径

- `forme-hostd --config` 明确安装/target/release、双 loopback listener、两个凭据文件与可选 worker/bootstrap 配置；closed schema、有界读取、路径分离。服务模式不从环境补值，不能开启普通进程 worker。
- gatewayd 从 CoreServiceConfig/ManagedBrainConfig 装配既有唯一 ProductionRuntimeBuilder。owner/store/workspace/model profile/凭据文件显式绑定；坏配置不回退环境或另建空脑。模型声明与实际 credential resolver 分开，实际 profile 继续进脑事件。
- webd 与 gatewayd 有独立 SCM dispatcher/停止接缝，Host 管理命令共用 execution 的内置 SCM 适配；gateway 只经既有 harness 边重导出，不加内部依赖。注册不启动 core；未认领时 core 按需启动，不反复失败重启。重复注册核对完整 image/参数/账户/依赖/启动与故障策略；移除要匹配绑定、观察到停服才删除。
- Web 身份只存独立认证库。服务 bearer 与请求摘要签名共同绑定安装、owner、会话/epoch、路由/body、期限及一次 nonce。core 的独立产品认证 fence 持久化撤销/nonce，重启不能复用已消费证明；不写脑事件/稳定认知。
- 浏览器只访问同源 Web API；Web 认证后转发 closed core 路由。正常对话仍经 RunRequest→Harness→context/model→events。审批继续绑定最终 plan；未知结果不能凭页面重试。
- React 工作台包含认领/登录、对话、项目、收件箱、认知、设置入口；对话支持 authoritative event cursor、重复/gap 处理、审批与取消。当前项目/认知主要是只读投影，设置主要是密码与健康，尚非 PRD 的完整五面功能。
- Node 只用于开发/构建；安装后 webd 提供编译嵌入的固定资源。exact lock、来源/依赖/许可证、每文件 hash 与 source digest 同步生成。没有导入 DeepSeek 源码、插件框架、后端或 fixture。

## 2. 打包与维护

`tools/build-product-bundle.mjs` 组合四个 binary、精确 Web 文件集、LICENSE/NOTICE 与各平台模板。输入先检查再创建新输出；拒绝现有输出、链接/路径交叠、改动/额外资源，复制前再检查输入摘要。输出是开发 inventory，明确 unsigned/native not performed/release_ready=false；目标枚举不证明二进制架构或该平台原生兼容。

Windows 使用固定账户矩阵，5/30/120 秒故障重启后停止自动重启；Linux 单元使用 Type=exec，进程运行与业务 ready 分开。Windows Inno、Linux systemd/deb/rpm、macOS launchd/pkg 仍是构建输入，不能单独当作可用安装包：首次配置/密钥预置、ACL/账户、启动与安装事务还须接入。

维护中实际拆出 harness ingress、production builder、capability 文档解析及 models file resolver。`production_config.rs` 从历史 841 行变为 493 行，解析子模块 356 行；删除该历史规模豁免。harness lib 仍有约 7390 行，store sqlite 仍约 8975 行；这些是公开债务，不能称已完成全仓大模块治理。新增文件依照 600/1200 行约定，旧债不增长。

## 3. 当前验证

本批完整源码门、命名组件门与核心回归结果在最终提交前补入。已取得的定向证据：

- 受管 Host 配置正反单元、真实进程测试：错误环境变量不影响文件配置；错误/相同凭据、跨域路径、非本机 target/绑定、文件超限/别名拒绝；错误配置不创建 journal。
- Web+Gateway+ProductionRuntime 真实 loopback：未认证/错误 CSRF 拒绝；同 key 仅一个 RunAccepted 和一次计数模型调用；证明 nonce 重放拒绝；注销后旧 cookie/旧 core assertion 拒绝；停止 Web 后原 run 继续并产生 RunComplete。模型为仓库 fixture，不能称真实 provider/value 验收。
- bundle 5 组回归覆盖六个目标的文件/hash、可重复性、旧输出保留、缺失 binary、篡改/额外/逃逸 Web 资源与链接拒绝；没有安装服务。
- 本机 Chrome headless 检查 webd 编译资源的未认领页：1440/390 像素布局无横向溢出、键盘 skip link、证明输入与浏览器必填校验、无页面脚本错误。检查发现并修复原生 fetch receiver 错误，并补回归。仅是初始化页浏览器证据，未覆盖已登录五面交互或无用户登录冷启动。

## 4. 明确未闭合

1. 全自动首次安装配置/密钥生成、账户/ACL、原生签名包、系统冷启动、卸载保留数据和升级恢复事务。
2. 逐 job 原生强隔离与桌面会话 helper；受管 core 的真实工具能力到隔离 worker 的接入。缺隔离时生产执行保持 blocked。
3. 模型/能力/channels 配置 staging/CAS/生效确认、secret 写入、core 离线维护；当前模型文件 resolver 是输入端，不是完整设置功能。
4. LAN TLS/证书/网卡/防火墙、旧脑或备份接管与 OS 管理员口令恢复；不能靠开发环境导入或匿名接口代替。
5. 附件/产物、项目/charter 编辑、候选纠正/撤销/遗忘、完整主动收件箱与维护设置；尚未实现的功能没有假成功按钮。
6. 已登录真实浏览器、真实 provider/工具、各原生平台与 IP24 全链路。历史模型/发布回执不自动覆盖本次产品源码。

当前交付是可继续集成的受管服务/Web 纵向切片；上述缺口包含未实现工作，不能全部写成“只剩测试”。
