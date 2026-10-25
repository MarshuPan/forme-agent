# 构建空间与缓存生命周期

本规范承接 `code-quality.md`、`prd/01 §9–10` 和 `prd/17`。仅管理开发机/CI 的构建资源，不改变 crate 图、运行时、事件、测试集合或验收结论。

## 1. 固定参数与复用

根 `Cargo.toml` 的 dev profile 使用 `debug = "line-tables-only"` 和 `incremental = false`；test 继承 dev。保留回溯文件名/行号、debug assertions、overflow checks 和 unwind。关闭增量编译会增加修改后的重编译成本，未变更的 Cargo 产物仍可复用。需要检查局部变量时，显式临时覆盖 debug，完成后恢复默认；不得把完整 debug 或增量编译写进个人环境后冒充默认门。

各 Git 工作树固定复用自己的 `target`。正式闭合/发布门继续要求 checkout-local `target/debug`，不得用全局共享 `CARGO_TARGET_DIR`、跳过测试或 release 测试替代。不要每次运行都创建带时间戳的新 target；确需隔离验证时，目录放在本工作树 `target/<任务名>`，保存证据后在整个任务的最后清理其 dev 产物。

CI 的 target 缓存身份包含根 `Cargo.toml` 和工具链；修改 profile 后不会命中或通过宽泛前缀恢复旧参数的构建输出。已有其他分支/工作树需合入本配置才会使用新默认值，仅清理缓存不会修改它们的源码。

## 2. 资源检查

`tools/build_resources.py check` 检查目标卷剩余空间；每个 Cargo 构建/检查/测试阶段开始前默认至少需要 20 GiB 可用空间。这是阶段前检查，单阶段仍会继续消耗空间。结束时的 `report` 给出构建缓存逻辑大小、硬链接去重后的分配大小和预算状态；初始每工作树缓存预算为 64 GiB，是报告阈值，不触发删除。阈值保存在 `tools/build-resource-policy.json`，后续根据完整回归实测调整。

```powershell
py -3 tools/build_resources.py check
py -3 tools/build_resources.py report --all-worktrees --json
```

报告只遍历注册工作树的 Cargo 缓存，不遍历其他磁盘。遇到链接、无法读取的文件时显式报告，不把不完整统计宣称为完整结果。环境覆盖项只报告变量名，不输出值。

## 3. 清理边界

`clean` 必须显式指定绝对 target 路径，默认仅运行 Cargo dry-run；`--apply` 才删除 dev 产物。仅接受本仓库注册工作树的 `target` 或有 Cargo 标记的一级任务 target，拒绝目录逃逸、junction/symlink/reparse point、非 Cargo debug 目录和仍在使用的目标。通过 Cargo 执行 `clean --profile dev --frozen --manifest-path ... --target-dir ...`；不自行删除 deps 中的哈希文件，不使用无参数的整 target 清理。

```powershell
py -3 tools/build_resources.py clean --target-dir C:/code/forme/target
# 在批准的维护窗口且目标无占用时应用：
py -3 tools/build_resources.py clean --target-dir C:/code/forme/target --apply
```

执行前后校验源码/配置、运行数据、研究语料及所选 target 中 dev 目录之外文件的内容摘要，保留发布目录、安装包和验收证据；`.rustc_info.json` 是可变 Cargo 元数据。进程检查后再 dry-run，并在实际删除前重查；Cargo 自身的目录锁继续生效。维护窗口内不得同时启动使用该 target 的新构建或独立测试进程。

临时 target 在整轮验证完成、证据落到 dev 目录之外后回收；已完成工作树只回收其缓存，源码和工作树登记保留。活跃工作区超过预算时先报告，在空闲维护窗口集中清理；不在每个测试后清空，也不在全局 finally 中无条件删除。

## 4. 变更验证

首次切换参数须在空闲时清理旧 dev 产物，再运行结构/格式/check/test/strict Clippy/合规及适用的永久门。记录清理回收、完整构建耗时、最终缓存大小和阶段采样峰值；采样峰值不能宣称为精确瞬时峰值。发布/原生安装/模型行为仍按原有证据单独验收。
