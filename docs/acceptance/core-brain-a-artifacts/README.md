# A 波入口与运行证据

`runtime.json` 记录冻结的 A 波逐名场景实际执行结果；`entrypoints.json` 来自同一 ProductionRuntime 中的 owner turn 与 scheduler 场景，保留连续 stream_seq、事件摘要、共同 SessionBound 配置快照和受控 HTTP 端点的实际调用数。

证据等级为 `ControlledRuntime`。这些记录证明正式装配、治理与入口一致性，不证明真实模型质量。`manifest.json` 绑定本轮源码输入、场景清单和文件字节摘要；独立校验使用 `tools/core_brain_evidence.py --verify-wave A --require-pass`。

通过 `tools/verify-core-brain-closure.ps1 -Wave A -EvidenceOutput <新目录>` 重新采集。不会导出 prompt、endpoint、密钥、owner 数据库或本机私有路径。
