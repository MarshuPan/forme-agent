# 工作空间纠正与真实遗忘预览验收（2026-11-09）

本记录承接 prd/26 §4.2、§5.6.2–5.6.3。边界说明提交为 `84f324d`，实现提交为 `d76b113`。真实已安装版本为 `forme-windows-x86_64-20261107-cf3fbc9`；本批源码修复尚未部署，不能关闭 IP20/IP24。工件根为当前工作树的 `target/native-legacy-review-20261106/`。

## 1. 实际安装的纠正与故障

浏览器只纠正隔离项目 `workspace:installed-project-20261108` 中的一个未提升候选。正式主脑将原候选降级，创建一条加密来源记忆及 Working/0.4 新候选，没有直接提升或改变权限。实际回执为 `correction:14ab6b1f-26ee-4a41-8a6d-2a7a5afe89b3`，Confirmed；其 binding 为 `sha256:2a801d06130ba382989cbf9a167ad6406bfd88f7364d6cacb607dad10680ff0b`。刷新后仍能查询原回执及来源，测试 script 标记作为文本显示，未形成可执行 script 元素。

随后从真实来源进入遗忘入口，正式 `PrepareLifecycle` 原命令 `data:installed-correction-prepare-20261108` 返回 HTTP 200，但业务回执为 Unknown、plan=null。未提交 Apply，未删除材料，没有更换命令号重试。网页刷新和“查询原操作”仍显示同一 Unknown，不能将 HTTP 成功解释成计划已准备。

2026-11-09 的独立只读复查仍得到原 Confirmed 纠正与原 Unknown 预览；四个原 run 的事件数量、顺序摘要、模型调用和动作数量保持。回执为 `installed-correction-original-command-20261109.json`，SHA-256 `846dfced94bcaa3d705159ffc6e238f8767ba5d3d83e4eea078a2e54e4ba9ec3`；其中绑定三份纠正观察、原预览失败和三张浏览器截图的摘要。

## 2. 同构复现与源码修复

新隔离测试使用默认工作空间以外的纠正来源，修复前明确失败为 `lifecycle key is shared outside approved subject`。两个实现问题共同造成这一条件：正文派生事件的加密 scope 回落默认工作空间；共用脑 aggregate 的空 session/transcript watermark 缓存保守依赖所有聚合事件，导致默认范围缓存被纳入项目来源的删除计划。原有测试只覆盖默认工作空间，未暴露此组合。

修复只在受限 owner correction 原子批次中为正文派生事件绑定已验证来源 scope，固定的无正文失效/回执事实保持原语义。内置缓存另须同时满足当前字节等于固定空结构及完整事件类型/schema 历史均为投影不变的封闭集合，才解除正文依赖；当前/历史正文、额外字段、错行或未知类型/schema 均不能取得该例外。生命周期的跨 scope 检查、旧密文、审批、one-shot、CAS 和原 Unknown 恢复规则不变，没有新协议字段、事件类型、crate 或依赖边。

继续检查发现，删除无正文 aggregate 的全部正文事件或尾部后，原缓存仍保留完整水位，而旧重放会回退到零或存活前缀。新增测试在修复前明确失败；修复后的 replay/rebuild 从同一快照恢复已证明无正文 stream 的物理审计水位，重建提交再次核对可读事件集合、水位与 upcaster。不能凭该水位恢复已删除正文，也不能向有正文/状态历史的 stream 扩张例外。原失败日志为 `correction-scope-tail-reproduction-20261109.log`，SHA-256 `c88ec89fe556c40f6286e341bb380940f749696b99c1e2d6687558a2e247a10c`。

## 3. 已取得的组件证据

- `installed_cognition_correction`：5/5。新增非默认工作空间路径经正式数据工作台 prepare/apply，删除后新正文候选不可检索，原降级及无正文纠正回执保留，另一工作空间原候选可读；重启后原 Apply 返回相同回执且事件水位保持。
- `v1_data_protection`：10/10；连同 schema replay 16/16、projection/FTS 9/9，共 35 项通过，见 `correction-scope-store-final-20261109.log`。包含实际跨工作空间引用拒绝、key/材料保留，以及删光/只删尾部后 replay、rebuild 和继续追加。随后增加的真正关闭/重开断言纳入最终整仓门，不能用这份先行日志替代。
- `owner_correction_atomic`：3/3；新增缓存判定单元反例 3/3，历史正文、当前正文、额外字段、错误绑定、陌生类型/schema 与缺历史不能绕过。后者日志为 `correction-scope-privacy-final-20261109.log`，该命令的 filter 未执行集成测试，不能另计一次 10/10。
- 独立 Web/Host/Core 进程与真实 core 重启用例：1/1，见 `correction-scope-process-20261109.log`。这是临时数据进程组件证据，不是固定安装已升级。
- 原生 Inno 成功/失败退出夹具：2/2，见 `correction-scope-inno-exit-20261109.log`，不安装或启停 ForMe。

最终源码的两项进程测试第一次并行执行时，纠正/真实 core 重启通过，managed configuration 在第二轮离线校验返回 502，整次为 1 passed / 1 failed；日志 `correction-scope-process-final-20261109.log` 保留。随后仅对本轮新建 fixture 增加只读耗时观察，原 managed configuration 全流程为 1/1、exit 0，日志 `correction-scope-offline-diagnostic-20261109.log`。其中正常 revision 2 校验为 200、6,801 ms；陈旧 revision 1 的 502 与非法来源的 400 是原测试预期拒绝。正常校验接近 Host 的 8 秒期限，但先前失败没有足够底层分类，不能把资源竞争或超时写成已证实根因。

整仓门结束后，原两项测试以 `node --test --test-concurrency=1 tools/tests/cognition_correction.test.mjs tools/tests/managed_configuration.test.mjs` 串行复验，实际 exit 0、2 passed / 0 failed；总耗时约 201 秒。日志 `correction-scope-process-serial-20261109.log` 的 SHA-256 为 `91b22d019b0f18b40c6d1761b22ed42665f2de0e9309a26fd2ef91a64ac6d433`。这是当前源码的真实临时进程/数据库证据，不能消除先行失败记录，不能当成长期稳定性或固定安装已升级的证明。

最终 `verify-code-quality.ps1 -WindowsInstallerToolchain <本机已核验配置>` 实际返回 0，日志 `correction-scope-quality-final-20261109.log` 的 SHA-256 为 `2cacdb8c68fc2fb225da2234d31b6eca6611e62170767e313cce6b108efac45a`。包含 fmt、check、当前全部 binary、183 个 Rust 测试目标（1,310 passed / 0 failed / 16 默认 ignored）、strict Clippy、前端 118/118、签名/包/PE/NTFS/防火墙只读夹具、实际 Inno 退出夹具 2/2 和完整合规。新尾部清理测试的关闭/重开断言已在该整仓测试中实际执行。结构门为 883 文件、63 项历史规模债务，未刷新基线。

冻结 workspace 门实际返回 0，18 crates、99 EventKinds、前 97 strict prefix 与精确依赖继续通过；日志 `correction-scope-workspace-20261109.log` 的 SHA-256 为 `c9d4c36167d40f131f334c593b518f0b00f46dfef25b40538df83f686a93391a`。最终空间报告为 `correction-scope-build-resources-20261109.json`，没有删除缓存。完整 M5/四波发布回执及安装 IP 是独立层级，本门不替代它们。

## 4. 安装与发布边界

原安装的 Host/Web/Brain 仍 Running，release-updater Stopped；本轮没有修改 Windows UAC、安装配置、程序或服务。当前工具进程没有管理员令牌。此前 `20261108-5ad46d4` 签名候选与密封批次不能覆盖本批新源码，不应将它作为本修复的安装证明。

后续须从新干净源码生成并核验候选，取得真实管理员窗口后完成受控部署及原生文件/数据联合验收。旧 Unknown 只查原命令；本修复不迁移历史密文、不恢复或重执行原预览。真实遗忘、备份恢复、桌面/受管消费者、跨设备及完整 IP24 继续保留各自未验证状态。

## 5. 闭合测试清单补齐与当前回归（2026-11-09）

后续完整 Python 回归在 `7b13529` 发现一项真实失败：`test_repository_inventory_contains_every_d_test_and_explicit_ignored_cli` 拒绝将本批新增的两个 store 数据保护测试遗漏在 D 波清单之外。原日志 `correction-scope-python-full-20261109.log` 保留于原工作树的工件根，SHA-256 为 `95964a7432cfec28365c66cf93eb7f261ef28121577db08a2e73a5c3ed68e3d4`；当次为 95 项、1 failed、1 skipped，不能用此前 Rust/质量门覆盖这次失败。

独立工作树 `shu/closure-inventory` 的修复提交 `037a9c8` 仅向 `C22-C23-data-protection` 登记以下两个真实测试；没有修改严格清单比对、测试 selector、Rust 实现、协议或规模基线。脚本仍为 600 行，A/B/C/D 清单分别为 47/50/36/73 项，总计 206；两项各出现一次。

- `c23_actual_cross_scope_content_dependency_still_rejects_without_deleting_keys`
- `c23_erased_aggregate_tail_keeps_replay_rebuild_and_subsequent_writes_consistent`

在与该提交相同的源码改动上，fmt、workspace check、全部 binary build、完整 workspace tests 与完整合规均通过。Rust 为 183 个测试目标、1,310 passed / 0 failed / 16 默认 ignored；完整 Python 独立运行为 95 项、94 passed / 1 skipped，原清单断言通过。结构门为 883 文件、63 项历史规模债务，`git diff --check` 通过。该回归不将默认 ignored、fixture 或旧安装升级为原生证据。

本次驱动在全部 Rust 测试结束后、严格 Clippy 执行前，因磁盘仅余约 16.1 GiB、低于规定的 20 GiB 而退出 1；Clippy 尚未执行，整套驱动不记 PASS。Python 和完整合规的上述通过结果来自独立进程。修复提交满足其格式、编译、完整 Rust 测试、结构和合规提交门；新源码的严格 Clippy、完整 M5、四波便携证据与 clean-tree Final 仍开放。

本轮日志在独立工作树 `target/closure-inventory-evidence/`，不是原生安装回执：

| 工件 | SHA-256 |
| --- | --- |
| `source-gates-native-part-01.log` | `d18966f17224f60ef7be96cf1010991ae8d4a70adc1022db5869373eaf268b7a` |
| `source-gates-native-part-02.log` | `430e42a73b427eb871d30f35269edb7ee0e69d15c9ead75b0a212dab0240d02c` |
| `python-full.log` | `d2ff3ae9eb030731945d83f8bd5d2900be11697b87d47999532c0e20180a8158` |
| `compliance-full.log` | `7902959834c14884d3ce0a23522df671eda274a2327a2020f961aef57f9148f5` |

`build-resources.json` 记录该工作树缓存约 25.3 GiB 逻辑大小、22.6 GiB 去重分配大小，预算未超限；它不代表目标卷仍有足够空间。主工作树缓存清理仅做 dry-run，尚未删除。

原工作树继续固定在干净 `7b13529`，保留已签名候选及原单次管理员批次；当前仍只有原 UAC 等待进程，无管理员启动、升级或产品窗口回执。该独立清单提交未改变它的 HEAD 或密封输入。旧 A/C24 证据不能重标为修复后完整四波证据：便携清单的 source attestation 同时包含 `crates`、`tools` 和 Cargo 输入，后续须按新清单重新采集。当前安装版本和 IP20/IP24 状态仍按 §4 保持。
