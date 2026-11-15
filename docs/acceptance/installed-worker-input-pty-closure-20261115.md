# Worker 输入与固定 PTY 接线闭合验收（2026-11-15）

本报告绑定当前工作树 `shu/product-integration` 的本轮未提交源码状态。它记录协议、Host resolver、worker 接线、固定 runner 和产品打包的组件结果；不把普通进程、机制 PTY、旧安装或交叉编译结果提升为原生产品验收。

## 结论

| 范围 | 判定 | 依据 |
| --- | --- | --- |
| WorkerInputGrant schema 与 exact binding | PASS | `forme-protocol` contract 场景 5 passed；grant 继续绑定 installation、owner、job/action、plan/permit digest、authority epoch、nonce、workspace、closed path 和期限。 |
| Host Content/Secret resolver | PASS | `installed::worker_input` 场景 3 passed；Content digest/长度、Secret slot owner/workspace/path/期限、逐项撤销与全量撤销均有回归。 |
| Host 分块读取 | PASS | `WorkerInputReadRequest` journal binding、状态、安装归属、offset/size/digest 检查通过；Content resolver 在读取路径再次核对 action/job binding。 |
| Worker stdin envelope | PASS | `installed_worker_process_contract` 6 passed；frame magic、大小、路径白名单、重复路径、secret marker、trailing bytes 和 EOF 行为已覆盖。 |
| 固定 PTY runner 策略 | PASS | runner 单元 3 passed；固定 `forme-pty-worker`、allowlist、workspace/cwd、清空环境、输入路径收敛、输出预算、secret redaction、timeout/overflow cleanup 和 `.pending` 原子发布已接线。 |
| 产品打包清单 | PASS | native package/product bundle/signed release 23 passed、1 skipped；8 个二进制均纳入清单，skip 是需要显式构建 `forme-hostd` 的真实 release verifier。 |
| Rust 整仓质量门 | PASS | `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、`cargo test --workspace --all-targets`（203 suites，1618 passed，38 ignored，9 filtered）通过。 |
| 代码质量与原创性合规 | PASS | `py -3 tools/code-quality.py`（1156 files，62 existing size debts）和 `bash tools/compliance-doctor.sh` 通过。 |
| 构建资源 | PASS | `py -3 tools/build_resources.py check` 通过；最终报告见 [`build-resources-report-20261115-final.json`](../../target/product-integration-evidence/build-resources-report-20261115-final.json)，logical 36,042,135,503 bytes、allocated 32,614,583,043 bytes，低于 64 GiB budget。 |

## 已完成的生产代码接缝

- `WorkerInputResolver` 使用 executor-local secret slot，支持单 slot 和全量撤销；解析失败、撤销、过期、绑定漂移继续 fail-closed。
- Content 只通过受管 resolver 解析，按 grant 声明的长度和 SHA-256 复核；Secret 值不进入 job、permit、journal、manifest 或普通继承环境。
- Process worker 与 Windows AppContainer worker 在 PTY profile 下发送一次性 frame；无输入时也发送明确 EOF，避免子进程等待未关闭的 stdin。
- `sandbox:installed-pty-v1` 只能调用仓库固定 runner；程序 allowlist 必须是成对的绝对路径，重复项和任意 runner 均拒绝。
- runner 只接受 PTY stdin/受管 secret environment，拒绝跨 backend 输入；输出受 `output_budget_bytes` 和 64 MiB 上限约束，超限或超时会尝试终止并等待 child，结果以同步 `.pending` 文件原子发布。
- `forme-pty-worker` 已进入 product binaries、native package、signed release、product bundle 和配置构建检查。

## 原生产品边界

以下项目本轮仍不能判定为 PASS：

- `supports_managed_pty()` 仍返回 `false`，因为当前分支没有真实通过审计的 Windows SCM/LocalSystem 服务身份、Low AppContainer SID/ACL、ConPTY/Job Object 进程树和独立终态回读。
- 当前安装仍是旧候选；没有把本工作树重新安装、升级或替换到正在运行的服务，也没有重新派发原来 `Validated / pending Quiescing` 的未知升级操作。
- secret 撤销/清理异常矩阵、真实 native PTY 正向/负向回执、桌面会话 helper 联合链和当前候选的 IP10、IP11、IP24 仍为 `UNVERIFIABLE` 或 `NOT_RUN`，详见[安装产品实施进度](installed-product-implementation-progress.md)。
- Linux/macOS 原生服务与发布边界仍未发布；WSL、Docker、普通进程或交叉编译不替代对应平台证据。

因此，本轮结论是“代码、协议和打包接缝完成，组件门通过；原生安装联合验收未完成”，不能写成整个安装产品或所有生产任务整体 PASS。
