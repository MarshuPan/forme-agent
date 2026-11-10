# SQLite 与 NOTICE 发布审计修复

2026-11-10，`a31ffbf` 的完整 M5 门在 S99 当前发布审计失败。原日志为 `target/product-integration-evidence/discovery-protocol-clean-m5.log`，SHA-256 `79a9c032e4720c16f446930d9bfbc97abc530dfd12da3d82232c45dc6f7f1848`。失败项是直接依赖 `png 0.18.1`、`crc32fast 1.5.0` 缺少 NOTICE，以及 Windows 发布目标可达的 `libsqlite3-sys 0.38.1` 命中 RUSTSEC-2026-0459。不是测试失败，也尚未运行到仓库发布回执比较。

## 实施范围

补齐两个已有直接依赖的声明。store/gateway 的 rusqlite 精确 pin 由 0.40.1 升至 0.41.1，lockfile 的 libsqlite3-sys 更新到 0.39.1，使用其 bundled SQLite 3.54.1。保留现有 backup/bundled/functions 特性，不新增内部依赖边，不改 store schema、协议或恢复权限。

本机 RustSec 数据库的 [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html) 指出：攻击者构造不匹配页大小的数据库与同目录 WAL 文件可能泄露未初始化内存；受影响版本止于 libsqlite3-sys 0.38.2，0.39.0 起修复。目标 0.39.1 bundled SQLite 为 3.54.1。当前 Rust 1.97 满足 rusqlite 0.41 的 Rust 1.89 要求。依赖仅通过公开 API 和构建产物使用，不复制其实现或测试。

## 验证与完成边界

更新前已取得 1,474 passed / 0 failed 的整仓结果，但不能作为新依赖验证。更新后必须重新运行 fmt、workspace/all-targets check/test、strict Clippy、质量和完整合规，并执行 M5 与 D runtime 门。需要重新核实原数据库/备份/恢复与产品认证测试，通过当前供应链审计后才生成新的发布回执；旧失败日志保留，不忽略公告、不放宽许可或漏洞规则。

本项是修复已发现的发布阻塞，不表示受管 MCP 执行链、桌面原生安装或 IP01–IP24 已闭合。后续组件验收记录实际测试、回执和剩余工作。
