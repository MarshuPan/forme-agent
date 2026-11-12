# 安装版声明式插件槽位

本文落实 owner 已批准的 `installed-managed-consumers-closure.md` §2–4，沿用原插件 registry、Host 配置写者与 ProductionRuntime；不引入可执行插件、额外 crate 或事件。来源不是 Web 上传、路径或 URL。

## 1. 固定来源

Host 从安装派生的 `core_config_path.with_extension("plugin-sources.json")` 读取独立 schema 1 `ProductPluginSourceCatalog`。该文件属于与 core 配置相同的受保护管理目录，只由本机安装/管理域提供；读取拒绝链接、未知字段、超过 256 KiB 和超过 64 个来源。不存在时目录为空，启用仍拒绝。公开视图只返回 source/plugin/manifest/scope 和摘要，不返回正文或路径。

`ProductPluginSource` 是 closed schema 1：source_ref、plugin_ref、manifest_ref、scope、skills。引用沿用 128 字节限制；skills 使用现有 ProductSkillConfig 的内容限制，至多 64 个且 id 唯一，必须与来源同 scope；来源内 enabled 必须为 false、trust 必须为 untrusted。来源不能自报可信或授权。来源摘要为 canonical_digest(("forme.product.plugin-source.v1", source))，同时包含全部正文与元数据。插件配置继续使用已批准的 ProductPluginConfig，不改变其形状与摘要。

schema 1 接通已有声明式 skill 槽位；schema 2 的工具和 hook 接入见 §5。未实现的 manifest 字段明确拒绝。M5 包状态仍只由原脑事件管理，不借本机来源目录创建或启用包。

## 2. 保存、发布和当前来源

启用需真实来源、精确 source/plugin/digest/scope、owner_input 信任，并验证整份来源与集合冲突。来源正文作为 Host 私有 ConfigState schema 13 的 `plugin_sources` 保存；缺省字段在旧 wire 省略。旧客户端和其他有限编辑保留此字段及已有较高 schema。保存先完成验证和整体 state/core 大小检查，再沿同一 journal publication；读取/保存均不启动程序或模型。

Core 私有投影为 `plugin_sources` 与 `plugin_configuration_path`。后者仅为安装派生的当前 core 配置，用于每次 context 装配前确认原配置没有禁用、更换来源或改变 owner。已保存的来源正文与其 digest 一并固定在原 revision，不在恢复时重新读取另一个来源。运行期在目录装配和每个模型轮次的自动技能准备路径同时复核当前安装来源目录，损坏/替换/禁用立即不再装配该插件内容；新版本需显式保存并受控重启。已加载的正文失去当前来源时，后续模型轮次拒绝继续使用；失败不会创建空 registry 来冒充有效来源。

插件转为原 LocalPlugin 的 Skill contribution，清单保持 Untrusted，信任只来自 owner 配置，grant 限于声明的技能与 scope、零执行 permission。与直接 skills 的 id 冲突提前拒绝。原 PluginContributionRegistered/PluginToggled 由原装配事件路径记录；不写第二份包状态。

## 3. 验证

覆盖正常来源→Host 保存→同一 ProductionRuntime 的 metadata/按需 body；默认禁用与不可信；错误摘要、来源、scope、版本、未知字段、超限、链接、重复 id；旧配置保存/恢复保持；运行期禁用和来源替换不得继续加载 body；过程中零外部执行。安装包、M5、其他插件槽位与真实渠道分别取得证据，不由本组件代替。

## 4. 有限设置外观

独立 schema 1 ProductPluginSettingsPatch 为 `{schema_version,operation_id,expected_revision,owner,plugins}`；摘要域为 `forme.product.plugin-settings.v1`。沿 MCP 有限编辑的同一 CAS、原 journal 和密码重新认证，不清空只写 publisher 材料。ProductPluginSettingsView 返回 owner/active_revision/pending_restart/configured/plugins/sources/sources_available；sources 是禁用且不可信的 ProductPluginConfig 安全摘要列表。来源目录损坏时 sources_available=false、sources 为空，既有配置仍可禁用或移除，不能因此困住撤销。Response 与 Operation 分别使用 `{schema_version,operation_id,applied,view}` 和 `{schema_version,owner,operation_id,request_digest,committed}`。输入 owner 只来自认证会话；GET/POST `/api/v1/product/plugin/settings` 与 GET `…/operations/{id}` 映射到 Host `/v1/host/configuration/plugin/settings`。未知字段、错摘要、CAS、重放、CSRF 和身份拒绝与 MCP 一致，有限编辑只改 plugins。

## 5. 既有执行绑定的声明式插槽

source schema 2 在同一私有来源对象上增加可选 `tool_slots`（空值省略，schema 1 字节保持）；每槽为 schema_version/id/binding/hooks，id 必须以 plugin-tool: 开头，binding 只引用现有正式工具 id。hooks 为最多 8 个 schema_version/id/point/transform，id 以 plugin-hook: 开头，point 为 before_policy/before_execution；transform 仅 identity、replace_capability 或 replace_shell_program，对应既有 HookTransform。每来源至多 64 个槽，与 skills 不同时为空；所有槽与 hook id 在启用来源集合中唯一。清单不能携带后端、执行权限、可执行文件路径、环境或自报 schema。

插件 Tool/Hook contribution 进入原 LocalPluginRegistry，仅授予声明式目录身份和零执行 permission。每个新 run 在已选定正式工具之后装配槽：目标缺失则该槽不可用；目标存在时复制目标 schema、scope、backend、risk 和 permissions，不能从插件取得新权限。MCP 槽复制原 MCP source/schema/settings pin，不触发发现。private run snapshot schema 2 保存这些插件 source/settings/slot pins，scope 与 composition 和原 SessionBound 一并摘要绑定；schema 1 原快照仍按原摘要读取。

槽与其全部 hook 同时经过原 CapabilityRegistry 在当前 session/scope/envelope 下的目录过滤。启用状态不替代目录准入；来源仍有效但 registry 已撤销、重新加载或改代时拒绝原装配引用，必须通过受控重装配确认新版本。

hook 在原 registry 中转换提议，在 policy/plan/approval 之前得到最终动作；capability/backend/scope/permissions 必须仍等于原目标 binding。before_execution hook 在最终执行检查时再次应用，若结果与原批准 intent 不同则拒绝，绝不在原 plan 上偷偷替换参数。每次准备及执行均复核来源配置、原来源目录和 source digest；禁用、替换、损坏或未注册 hook 阻断原槽动作。普通工具不获得 discovery 能力门例外。
