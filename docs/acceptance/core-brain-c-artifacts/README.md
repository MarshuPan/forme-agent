# C 波项目与编排证据

本目录的六份 versioned JSON 由 repository-owned fixtures 生成，源码基线为 `db29cbb83f044eff8baf05f08e179295de229574`。证据等级固定为 `controlled-http-real-runtime-sqlite-files`：受控 HTTP 模型、正式 ProductionRuntime、真实 SQLite、文件和 Gateway 子进程。它们不证明线上模型质量、某种编排优于直接模型或长期主动价值。

| 文件 | 证明范围 |
|---|---|
| `patterns.json` | 八套模式进入正式运行时，前四套零 child，后四套按依赖执行。 |
| `conflict.json` | 事实、设计和质量冲突保留并上抛；取消不洗白结果。 |
| `recovery.json` | 未知副作用跨重启持久，保留预算预留，零自动重试。 |
| `isolation.json` | 真实子目录文件工具，审批前零副作用，批准后一次，兄弟目录隔离。 |
| `supervision.json` | checkpoint 后由主脑监管新证据并建立下一 segment，不热换运行中路线。 |
| `learning.json` | 成功与失败都回流 episode、lineage 和 strategy candidate，active fitness 不变。 |

每份包含 composition/taxonomy digest、实际模型 profile 映射、完整 stream_seq/hash 见证、安全载荷白名单、在线 ledger digest 与相对路径文件 hash。省略的敏感载荷仅保留 hash 见证，不能从工件恢复；这不是完整 owner 数据备份。离线校验器无需模型凭证或 owner store：

```powershell
cargo run -p forme-eval --bin forme-core-brain-c-artifact-verify -- docs/acceptance/core-brain-c-artifacts/patterns.json
```

校验器重放编排事实、核对预算/调用次数/终态/候选及文件证据，拒绝缺失序号、载荷遗漏、digest/预算/profile/schema 篡改、未知字段、秘密和本机路径。文件内容 hash 是捕获时的证据，离线校验不读取原本机文件。

重新生成方法：

```powershell
$env:FORME_CORE_C_ARTIFACT_DIR = 'C:\code\forme\docs\acceptance\core-brain-c-artifacts'
try {
    cargo test -p forme-harness --test v1_core_brain_c_orchestration -- --test-threads=1
} finally {
    Remove-Item Env:FORME_CORE_C_ARTIFACT_DIR -ErrorAction SilentlyContinue
}
```

应先提交待验证源码，再生成工件，确保 `source_commit` 指向实际实现。时间、事件 ID 和本地路径 hash 会随运行变化，不要求两次工件字节相等。`tools/verify-core-brain-closure.ps1 -Wave C` 在临时目录重新生成六例，同时独立验证新工件与仓库工件；不会覆盖本目录。
