# EveTools

EveTools 是一个桌面优先的 EVE Online 空间站交易辅助工具，当前聚焦 Jita 4-4。

当前实现切片包含：

- Tauri 2 桌面壳
- React/Vite 桌面 UI
- 面向领域逻辑、ESI、Supabase catalog 数据和 worker 的 Rust workspace crates
- 已测试的 Rust 领域计算：价差、手续费、流动性和关注度评分
- 基于公开 ESI 的市场价格查询，并带 fixture fallback
- 基于公开 ESI 的选品发现，并带 fixture fallback
- 基于 fixture 的订单监控

当前切片在可用时使用公开 ESI 做市场查询和选品发现，同时保留确定性的 fixture fallback 以便开发和故障降级。静态 SDE catalog 数据通过 Rust catalog service 导入 Supabase Postgres。EVE SSO 和认证角色订单同步会在后续实现阶段处理。

## 开发

安装依赖：

```sh
pnpm install
```

运行全部检查：

```sh
pnpm check
```

只运行 Rust 测试：

```sh
pnpm test:rust
```

只运行 TypeScript 类型检查：

```sh
pnpm typecheck
```

启动桌面应用：

```sh
pnpm dev
```

构建桌面应用：

```sh
pnpm build
```

## 公开 ESI 市场同步

桌面应用可以使用公开 ESI 数据驱动 Jita 市场查询和选品看板。

市场数据源由 `EVETOOLS_MARKET_SOURCE` 控制：

```bash
EVETOOLS_MARKET_SOURCE=live pnpm dev
EVETOOLS_MARKET_SOURCE=fixture pnpm dev
```

未设置该变量时，后端默认使用 `live`。

公开 ESI 模式当前使用这些无需认证的 endpoints：

- `POST /universe/ids/`
- `GET /universe/types/{type_id}/`
- `GET /markets/{region_id}/orders/`
- `GET /markets/{region_id}/history/`

当前公开数据切片刻意保持较小范围：

- 只覆盖 The Forge 区域。
- 只使用 Jita 4-4 空间站订单做 top-of-book 分析。
- 选品发现使用固定种子池。
- 公开 ESI 网络、状态码或解码失败时使用 fixture fallback。

认证角色订单监控在 SSO 阶段前仍由 fixture 驱动。

## 静态 SDE Catalog

EveTools 通过 Rust catalog service 将 CCP 官方 SDE JSON Lines 压缩包导入 Supabase Postgres。

### 数据库连接

启动桌面应用前，在本地 shell 中设置 catalog 数据库 URL：

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
pnpm dev
```

连接串从 Supabase Dashboard 的 `Connect` 面板获取。选择 direct connection 和 pooler 时，可参考 Supabase 的 [database connection guide](https://supabase.com/docs/guides/database/connecting-to-postgres/serverless-drivers)。对于 catalog 导入，优先使用以下连接方式：

- 启用 SSL 的 Direct Postgres connection。这是本地/admin 导入的首选方式，因为 importer 会执行 migration 和长事务。
- 如果本地网络无法访问 direct IPv6 endpoint，可以使用 Supavisor session pooler。
- 不要把 transaction pooler 用于这个 importer。Importer 使用长事务和 `sqlx`；transaction pooling 更适合短生命周期/serverless 流量，并且可能与 prepared statement 行为冲突。

URL 必须启用 SSL。没有 query 参数时使用 `?sslmode=require`；如果 URL 已有 query 参数，则追加 `&sslmode=require`。如果你已在 Supabase 启用 SSL enforcement，并在本地安装了项目 CA 证书，`sslmode=verify-full` 更强。

需要让 repository integration tests 真实访问 Postgres 时，使用单独的测试 URL：

```bash
export EVETOOLS_TEST_DATABASE_URL="<dev-or-test-supabase-postgres-url-with-sslmode-require>"
cargo test -p evetools-db --test catalog_repository -- --nocapture
```

未设置 `EVETOOLS_TEST_DATABASE_URL` 时，Postgres integration tests 会自动跳过。Importer 拥有 `evetools_catalog` schema，并会在每次成功导入后替换 catalog rows，所以测试请使用开发用或可丢弃的 Supabase project。不要让 `EVETOOLS_TEST_DATABASE_URL` 指向保存完整 catalog 的同一个数据库，否则测试样例会把完整 SDE 数据替换成一行 fixture。

需要手动初始化或修复完整官方 SDE catalog 时，使用 admin CLI：

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
cargo run -p evetools-catalog --bin import-sde-latest
```

这个 CLI 只是薄入口：读取 `EVETOOLS_DATABASE_URL`，调用 Rust `CatalogService::import_latest()`，并输出导入状态和行数。正式维护时可以把同一套 service 方法接到 hosted job、worker、GitHub Actions 或 Supabase 托管函数上；不要把特权数据库连接串分发给最终用户桌面端。

CLI 会显示阶段级进度：检查 SDE metadata、检查当前 catalog、下载完成大小、解析后的行数、写入 Postgres 的分表行数，以及最终状态。下载阶段第一版不显示百分比；写库阶段每 1000 行和每张表最后一行报告一次。

SDE 实体名和描述会导入到标准化 localization 表中。`get_inventory_type` 和 `search_inventory_types` 在服务端根据请求语言选择显示名，前端只传当前语言，不解析 SDE 多语言 JSON。语言 fallback 顺序为精确语言、基础语言、中文 fallback、英文 fallback，再退回任意可用名称。升级到包含 localization 表的版本后，需要重新运行一次完整 SDE 导入来填充这些表。

不要提交真实数据库 URL 或密码。不要把它们放进已纳入版本控制的 `.env` 文件。如果凭据出现在聊天、日志、截图或源码控制中，请先在 Supabase 中轮换后再使用。

Direct Supabase Postgres 模式只适用于本地、私有或 admin catalog 导入。`EVETOOLS_DATABASE_URL` 是特权凭据：不要把它打包进 Tauri app，不要注入给最终用户，也不要要求最终用户持有它。正式分发前，必须把直接数据库访问替换为 hosted API、Supabase Edge Function，或严格由 RLS 约束的只读路径。

第一版 catalog 切片导入：

- `_sde.jsonl`
- `types.jsonl`
- `groups.jsonl`
- `categories.jsonl`
- `marketGroups.jsonl`

React 不直接连接 Supabase。React 调用 Tauri commands，Tauri 再调用 Rust catalog service。

## 架构

业务逻辑位于 Rust crates：

- `crates/domain`：市场模型、价格计算、评分、序列化 view models 和 fixtures。
- `crates/esi`：ESI client 边界。
- `crates/sde`：SDE JSON Lines 压缩包发现和记录解析。
- `crates/db`：Supabase/Postgres catalog schema 和 repository。
- `crates/catalog`：用于导入和查询静态 SDE 数据的 Rust catalog service。
- `crates/worker`：同步状态和 worker 边界。

桌面应用位于 `apps/desktop`：

- `apps/desktop/src`：React UI 和 typed Tauri command wrappers。
- `apps/desktop/src-tauri`：Tauri 2 Rust crate 和 command handlers。

React 渲染后端准备好的 views，并调用 Tauri commands。Tauri commands 是 Rust crates 的适配层；交易计算应保留在 `crates/domain` 中。

## MVP 界面

第一个桌面屏幕包含三个界面：

- `Market Price Lookup`：查询某个物品当前 Jita 价格状态。
- `Selection Discovery`：列出候选物品及入场价、出场价、净利润、评分和理由。
- `Order Monitor`：展示类似活跃订单的 fixture rows，并给出建议动作和紧急度。

同步状态分为公开和私有流程：

- Public market sync：`live-ready`、`fixture-ready` 或 `fixture-fallback`
- Authenticated order sync：`not-authorized`
- Data source：`live` 或 `fixture`

## 范围

当前 foundation 范围内：

- 本地 Tauri 桌面壳。
- 基于公开 ESI 的市场查询和选品发现。
- Fixture fallback command 边界。
- 连接到 Tauri commands 的 React UI。
- 确定性、可测试的 Rust 领域计算。

当前 foundation 范围外：

- Supabase 静态 SDE catalog 之外的完整交易持久化。
- EVE SSO token 处理。
- 认证角色订单同步。
- 自动下单、改单或撤单。
