# V1 大脑：数据控制与 D 波真实验收操作

本手册对应 `prd/23` D 波、canonical §28。正式 CLI 的 data/dashboard 命令通过已认证的 Gateway 使用同一个 ProductionRuntime；它们不另开 SQLite writer。离线旧明文迁移是唯一独立 operator，必须先停止 authority daemon。

## 1. 模型配置

普通单模型会话兼容 `FORME_MODEL_BASE_URL`、`FORME_MODEL_NAME`、`FORME_MODEL_API_KEY`。需要工具调用和多模型时使用 `FORME_MODEL_PROFILES_JSON`，它优先于旧三变量，且只引用密钥变量名：

```powershell
$env:FORME_MODEL_PROFILES_JSON = @'
[{"id":"primary","provider":"compatible","model":"实际模型名","base_url":"https://你的兼容端点/v1","credential_env":"FORME_MODEL_API_KEY","tool_use":true,"tool_schema_versions":[1],"default":true}]
'@
$env:FORME_MODEL_API_KEY = Read-Host 'API Key' -MaskInput
```

`-MaskInput` 需要 PowerShell 7。当前终端的 `$env:` 不会反向传播给已经运行的 Codex/daemon；如需另一个进程加载，可通过 Windows User 环境保存配置，在该进程内重新读取，或由 owner 提供本机受保护的配置加载方式。不要把密钥放进 JSON、Git、命令参数或验收工件。

如果选择 Windows User 环境作为本机配置来源，在填好上述两个 `$env:` 后保存：

```powershell
[Environment]::SetEnvironmentVariable('FORME_MODEL_PROFILES_JSON', $env:FORME_MODEL_PROFILES_JSON, 'User')
[Environment]::SetEnvironmentVariable('FORME_MODEL_API_KEY', $env:FORME_MODEL_API_KEY, 'User')
```

新启动的进程会继承配置；已运行进程的验收子进程应在内部用 `[Environment]::GetEnvironmentVariable(<变量名>, 'User')` 加载，禁止打印值。若 `credential_env` 使用其他变量名，加载相应变量，不能假定都叫 `FORME_MODEL_API_KEY`。这里不需要修改 Rust 源码，当前入口也不自动读取 `.env`。

fallback 只有显式相同非空 `fallback_group` 的兼容 profile 才可参与，最多 primary + 2 fallback；高风险/L5、结果不明和链耗尽都停止。每次 attempt 记录实际 profile；配置过 fallback 不代表发生过 fallback。默认价格未知时费用为 unknown，不把零配置价格当作免费。

## 2. 备份、校验与恢复

先连接已启动的 daemon：配置 `FORME_GATEWAY_URL`，并使用 daemon provision 的 token 文件（`FORME_GATEWAY_TOKEN_PATH`）或本地 `FORME_GATEWAY_TOKEN`。CLI 自动携带 Bearer、Origin 与 CSRF。文件名仅允许不含路径分隔符的 opaque 名称，落在 authority DB 旁的 `data-operations/` 下。

```powershell
'{"schema_version":1,"target":"owner-snapshot.db"}' | cargo run -p forme-cli -- data backup
'{"schema_version":1,"source":"owner-snapshot.db"}' | cargo run -p forme-cli -- data verify-backup
'{"schema_version":1,"source":"owner-snapshot.db","target":"owner-restored.db"}' | cargo run -p forme-cli -- data restore-plan
```

恢复先取得并人工核对完整 plan。确认时必须原样提交 plan，并由确认内容绑定 `digest`、`nonce`、`expires_at`：

```powershell
# $plan 为上一条 restore-plan 的完整 JSON 响应，owner 已核对 source/target。
$request = @{
  schema_version = 1; plan = $plan
  confirmation = @{ schema_version = 1; plan_digest = $plan.digest
    nonce = $plan.nonce; use_by = $plan.expires_at; approved = $true }
}
$request | ConvertTo-Json -Depth 64 -Compress | cargo run -p forme-cli -- data restore
```

备份使用 SQLite backup API，带 manifest、DB digest、事件连续性及 replay/projection 对账。恢复只写新目标，不自动切换 authority；须停止旧 daemon，由 owner 显式选择恢复 DB，再重启唯一 writer。恢复不能覆盖已有文件。one-shot 已消费但结果不明时先检查目标与审计，不重复 apply。

数据库备份**不含内容密钥**。生产密钥目录为 `FORME_STORE_PATH` 对应路径的 `.content-keys` 扩展名目录；离线保护该目录并与数据库分开管理，恢复必须仍能访问正确 resolver。复制旧 key 会削弱 crypto-shred 的删除保证；不要把 key 目录作为普通工件或备份附件发布。

## 3. 删除与保留

生命周期 Prepare 的参数为 `{action:"Prepare",schema_version:1,scope,subject_digest,operation,expected_version}`；operation 为 `Retain`、`Delete` 或 `CryptoShred`。subject 为被清理内容原聚合/来源的安全 digest；scope 必须是该 owner 授权范围，expected version 初次为 0，此后取上次成功 receipt 的 committed version。不得对不明对象猜测 scope/version 或直接删除 DB 文件。

```powershell
# $previewRequest 是 owner 已确认的 Prepare 请求。
$plan = ($previewRequest | ConvertTo-Json -Depth 64 -Compress |
  cargo run -p forme-cli -- data lifecycle | Out-String) | ConvertFrom-Json
```

先核对 `plan.operation` 内列出的对象、派生血缘和受影响的同聚合依赖后缀。批准后才构造与恢复相同的 confirmation，并以 `{action:"Apply",schema_version:1,plan,confirmation}` 调用 `data lifecycle`。不得重新计算 digest 或替换对象列表。subject 在预览后有新内容、version 改变、授权过期时重新 Prepare。

删除依次固定 pending plan/安装 fence/移除投影、销毁并验证对应历史 key、最后追加 `DataLifecycleApplied` 与 tombstone。没有成功 receipt 就没有完成；中断后保留同 digest 的 pending job，用仍有效的原授权恢复，授权失效则重新取得批准。pending 时所有正式读/执行入口和缓存读取停止。活动或挂起 run/route 必须先解决，清理不抢占正在执行的动作。

成功后 memory/cognition cache 刷新；旧事件密文和审计元数据仍保留，旧 DB 备份也不能解出已经销毁 key 的内容。远端未核验为 Unknown。当前没有独立持久 vector index，不声称清除了不存在的索引。物理扇区、系统内存、用户另存的原文/密钥不在本地应用层删除证明内。

## 4. 旧明文数据库迁移

已有明文 DB 不能静默与新密文混存。先停止 daemon，保留源 DB，确认 `FORME_OWNER_ID` 与源 `brain_authority` 的 owner 一致，并设置正确 `FORME_WORKSPACE`。目标必须是新文件，目标 `.content-keys` 目录也必须不存在。

```powershell
$planJson = cargo run -p forme-store --bin forme-data-migrate -- plan .\old.db .\protected.db | Out-String
if ($LASTEXITCODE -ne 0) { throw '迁移预览失败' }
$plan = $planJson | ConvertFrom-Json
$plan | Format-List
```

人工核对 owner/source/target/schema digest、event/content 数与有效期后，在同一有效期内 apply：

```powershell
$planJson | cargo run -p forme-store --bin forme-data-migrate -- apply .\old.db .\protected.db $plan.digest
```

plan 不创建 DB/key；apply 重新校验源字节、scope/owner、批准 digest 和目标，使用一致性快照完成全部已知内容列加密、空闲页压实、checksum/integrity/replay 验证。成功 receipt 明示 `source_unchanged=true`、`source_deleted=false`、`authority_switched=false`；旧源仍是**未保护备份**。失败目标可能仍含明文，不能作为完成迁移；源和失败目标都继续按敏感数据保护，重试使用新的目标，不覆盖旧文件。迁移成功也不自动清除旧备份或接管 authority。

## 5. 认知看板

```powershell
cargo run -p forme-cli -- brain dashboard
cargo run -p forme-cli -- brain dashboard workspace:default
```

HTTP `GET /v1/brain/dashboard?workspace=...&from=...&to=...` 也支持时间窗（Unix 毫秒）。窗内行为与截至窗尾的 backlog/confidence/supervision 状态分别核算；每 stream 的 heads 是权威游标，单个最大序号只作显示。问答被接受不代表学到的知识有用，reject 不等于 regret；没有结果证据的指标标 unverifiable。看板可由事件重建，不成为第二事实库。

## 6. C24 真实任务与发布门

配置真实、支持工具调用的模型后，从干净源码提交运行到**新的目录**：

```powershell
cargo run -p forme-harness --bin forme-core-brain-d-golden -- --live --out .\c24-evidence --repeats 2
cargo run -p forme-eval --bin forme-core-brain-d-artifact-verify -- .\c24-evidence\product-golden.json --require-pass
```

六类固定任务覆盖冷启动、工具写文件、跨 SQLite 重启的长期 workspace、多个 child、AskToLearn、失败后重协调。每类至少两次，直接模型只作同一事实输入的答案质量基线；文件/route/checkpoint/候选的 workflow 结果另由独立 selector 证明。该短周期套件不代表真实用户数月收益或通用复杂任务能力已经得到证明。

缺配置会生成 `Unavailable/UNVERIFIABLE` 工件并返回 2；controlled transport 不能成为 live 成功。仅 `-Wave D -RuntimeOnly` PASS 证明实现与治理回归，不能声明 D/C24/Final PASS。正式工件置于 `acceptance/core-brain-d-artifacts/product-golden.json`，再执行：

```powershell
pwsh -NoProfile -File tools/verify-core-brain-closure.ps1 -Wave D
pwsh -NoProfile -File tools/verify-core-brain-closure.ps1 -Wave Final
```

Final 还要求 A–C、S1–S99、fmt/check/test/strict-clippy/compliance、当前干净树 release audit。发布检查只能用该 checkout 的 `target/debug`；不得借共享 target、旧 receipt 或自报最终回答通过。分支实现、真实价值通过和 main 合入是三个单独状态。

## 7. 分波证据清单与最终汇总

`verify-core-brain-closure.ps1 -Wave A|B|C|D -EvidenceOutput <新目录>` 在逐名测试全部实际通过后，导出该波 `runtime.json`。其中只记录冻结场景身份、恰好一次执行的计数、源码提交和输入摘要，不导出完整 shell 日志。A 波同时导出正式 owner turn/scheduler 的脱敏事件位置、共同 SessionBound 快照和模型端点实际调用数；B 波导出三相 tick 的事件位置、phase 顺序、report/认知快照摘要与实际模型调用计数。这些是受控 HTTP 运行证据，不冒充真实模型质量。C 波同时导出本次重新生成的六份 typed 工件。

A 波的 owner turn 必须有 context/model 完整往返；`Schedule` 通知 run 必须有主动门、策略、action 和 verification 完整执行。后者本来就不需要调用模型；两者共同配置快照相同不代表事件序列应完全相同，实际模型调用计数必须与端点一致。

将运行报告与各波 typed 工件放入 `core-brain-{a,b,c,d}-artifacts/` 后，由 `tools/core_brain_evidence.py --capture-wave A|B|C|D` 生成 `manifest.json`。清单采用封闭字段与文件集合，绑定源码/场景清单/输入摘要、每个文件的字节摘要、证据等级、事件范围和配置/模型/策略快照引用，以及秘密/私有路径扫描结果。`README.md` 是唯一可选说明文件，也必须纳入摘要；不允许额外文件、嵌套目录、符号链接或未知字段。

这些工件目录和首次真实试验归档由 `.gitattributes` 设置 `-text`，Git 必须保留原始字节；不能让自动换行转换改变清单中的文件摘要。

`--assemble` 生成 `core-brain-closure-artifacts/manifest.json`，只引用四波清单及其摘要和共同源码输入，不把报告或发布 receipt 纳入自身，避免循环摘要。`--verify` 离线校验封闭集合、全部引用、源码与执行清单一致性；源码验真需要本地 Git 历史，不发起模型/工具副作用。typed B/C/D 内容仍由原有独立 Rust verifier 判定，不能只凭 JSON 中的 Pass。

D 的 `runtime.json` 即使通过，缺少 live C24 时 D 清单与汇总仍必须是 `Unverifiable`。正式 D/Final 和 release audit 额外要求这些清单、独立 typed 校验与 C24 全部通过。证据包只证明绑定源码及有界场景；不证明真实用户数月收益，也不是签名远端证明。历史波次报告不回写；当前源码重验写入新清单。

## 8. 默认大脑与 M3 控制

同一 owner 的 workspace 是共享大脑下的隔离项目分区。先通过既有 charter 接口登记工作区，再登记/规划项目。登记不授予文件、网络、工具或子代理权限；不同 scope 的画像、信任和结果证据不能相互抬高能力门。实际模型上下文读取该 scope 中仍然有效的稳定认知正文，候选/撤销/失效内容不作为稳定判断输入。

没有 active strategy 时默认 runtime 使用内置行为，不阻断普通任务。首次升级前，owner 通过 `POST /v1/evolution/seeds` 提交 `{schema_version:1,domain:"Loop",workspace:"workspace:default"}`（domain 也支持 Coordination、CapabilitySelection、ModelSelection、BackendSelection、ModelAdaptation）。接口要求与其他 mutation 相同的 Bearer、Origin 和 CSRF。初始化记录内置行为与当前编译实现的等价性，不是模型质量提升证据；重复初始化不重置已激活策略。

`POST /v1/evolution/specs` 接收 `{schema_version:1,spec:<ProductionStrategySpec>}`，返回封存后的正文、内容引用和摘要。正文登记不改变 active policy；同 version 的不同正文被拒，升级必须新 version。随后原有 candidates/evaluations/promotions/activations 接口仍依序走 governor、独立评估和 owner 控制。候选必须匹配返回的 domain/scope/version/ref/digest；没有正文或参数被替换时拒绝。候选事件进入 manifest 的 `aggregate_refs.memory_cognition`，用返回的 event id 在该流查询。

路线成功和失败都会沉淀为 episode/候选，但只有某个已激活 Coordination 策略明确绑定的、早于候选快照的同类任务证据，才影响后续模式适配分数。晚到或未激活经验不改变运行中策略。第一次升级可以回滚到已登记的内置 seed；seed 绑定编译实现摘要，不跨不相容构建静默复用。

永久闭合门将七项补闭检查分别归入既有 C1–C24，不另造里程碑或场景编号。受控 HTTP、真实文件操作及重启证据证明接线与治理；真实 C24 六类任务另外重采，不把本节检查当作长期学习收益证明。
