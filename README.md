# EveTools

EveTools 是一个桌面优先的 EVE Online 空间站交易辅助工具，当前聚焦 Jita 4-4。

当前实现切片包含：

- Tauri 2 桌面壳
- React/Vite 桌面 UI
- 面向领域逻辑、ESI、Supabase catalog 数据和 worker 的 Rust workspace crates
- 已测试的 Rust 领域计算：价差、手续费、流动性和关注度评分
- 基于 hosted API 和 Supabase 快照的市场价格查询，并带 fixture fallback
- 基于 hosted API 和 Supabase 快照的选品发现，并带 fixture fallback
- 基于 fixture 的订单监控

当前切片由 worker 使用公开 ESI 同步市场订单到 Supabase；桌面端通过 hosted HTTP API 读取最新成功快照做市场查询和选品发现，并保留确定性的 fixture fallback 以便开发和故障降级。静态 SDE catalog 数据通过 Rust catalog service 导入 Supabase Postgres。EVE SSO 和认证角色订单同步会在后续实现阶段处理。

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

### 企业级测试基线

默认测试分三层：

- 纯 Rust/TypeScript 单元测试：不需要数据库，`cargo test --workspace` 和 `pnpm typecheck` 会直接运行。
- Postgres integration tests：只允许连接本地一次性 Postgres，测试开始会删除并重建 `evetools_catalog` schema 和 SQLx migration metadata。
- 远程 Supabase 验证：默认禁止，只有人工确认要破坏性测试远程库时才显式开启。

推荐使用仓库内的本地测试库：

```bash
docker compose -f docker-compose.test.yml up -d
export EVETOOLS_TEST_DATABASE_URL='postgresql://postgres:postgres@127.0.0.1:54329/evetools_test?sslmode=disable'

cargo test -p evetools-test-support
cargo test -p evetools-db --test catalog_repository -- --nocapture
cargo test -p evetools-db --test market_repository -- --nocapture
cargo test -p evetools-worker --test public_market_sync -- --nocapture
cargo test --workspace

docker compose -f docker-compose.test.yml down -v
```

`EVETOOLS_TEST_DATABASE_URL` 如果指向非本地主机，测试会直接失败，避免误把完整 Supabase catalog 用 fixture 覆盖。只有在你确实准备好使用可丢弃远程库时，才可以临时开启：

```bash
export EVETOOLS_TEST_DATABASE_ALLOW_REMOTE=1
```

## 公开 ESI 市场同步

桌面应用通过 hosted HTTP API 使用 Supabase 中的最新成功市场订单快照驱动 Jita 市场查询和多交易点 Selection Discovery。后端具备第一版 Supabase 市场订单同步基础，可按主流 NPC trade hub 过滤并保存公开区域订单快照。

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

当前桌面 UI 公开数据切片刻意保持较小范围：

- Market Price Lookup 默认读取 Jita 4-4 最新成功订单快照。
- Selection Discovery 支持 Jita、Amarr、Dodixie、Rens、Hek；需要先同步对应 region 才会显示该交易点的快照推荐。
- Selection Discovery 默认读取最新成功快照，不再使用固定 type_id 种子池，并可在 UI 中按交易点筛选。
- hosted API 不可用、本地未配置 API base URL 或未同步快照时使用 fixture fallback。

市场订单持久化切片已经定义以下 NPC trade hubs：Jita、Amarr、Dodixie、Rens、Hek。Worker 层会从 `GET /markets/{region_id}/orders/` 拉取区域公开订单，按这些 station ID 过滤后写入 Supabase 的 `trade_hubs`、`market_sync_runs` 和 `market_order_snapshots` 表。Market Price Lookup 会解析本地 catalog 物品并读取 Jita 最新盘口；Selection Discovery 会聚合最新成功快照中的 type 级买卖盘口，按价差、净利润、流动性和置信度排序。

认证角色订单监控在 SSO 阶段前仍由 fixture 驱动。

### 同步区域市场订单

可以用 worker CLI 手动同步某个 region 的公开市场订单。第一版会拉取 region 级公开订单，然后只保留已配置 trade hub station 的订单快照。

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
cargo run -p evetools-worker --bin sync-public-market-region -- --region-id 10000002
```

不传 region 时默认同步 The Forge：

```bash
cargo run -p evetools-worker --bin sync-public-market-region
```

同步第一版全部主流 NPC 交易点：

```bash
cargo run -p evetools-worker --bin sync-public-market-region -- --all-default-regions
```

CLI 会运行数据库 migrations、写入/更新默认 trade hubs、创建 `market_sync_runs`，并把过滤后的订单写入 `market_order_snapshots`。成功后会输出本次 `sync_run_id`。`--json` 会输出不包含数据库连接串的 summary 数组，便于 scheduler 或 CI job 解析。`EVETOOLS_ESI_BASE_URL` 仅用于本地测试或 mock ESI，不需要在正常使用时设置。

### 生产同步任务

生产环境中，桌面端不运行同步任务。公开市场同步由服务端 scheduler 触发 `evetools-worker`：

```bash
export EVETOOLS_DATABASE_URL="<worker-postgres-url-with-sslmode-require>"
cargo run -p evetools-worker --bin sync-public-market-region -- \
  --all-default-regions \
  --started-by production-scheduler \
  --lease-ttl-seconds 1200 \
  --max-age-seconds 600 \
  --json
```

生产后端可以通过 GitHub Actions、S3、SSM 和 GitHub OIDC 部署到固定 EC2 实例。部署准备、IAM 权限、systemd 服务和手动验证步骤见 [AWS S3 + SSM + OIDC 部署指南](docs/deployment/aws-s3-ssm-oidc.md)。

`--all-default-regions` 会按顺序同步 Jita、Amarr、Dodixie、Rens、Hek 所在 region。Worker 会为每个 region 获取数据库 lease；如果另一个 worker 已经在同步同一 region，本次运行会返回 `already-running` 并以成功退出，避免 scheduler 因正常锁竞争误报失败。`--max-age-seconds` 用于跳过仍足够新的快照，减少 ESI 请求和数据库写入。

本阶段不接入监控报警平台。生产健康状态通过 HTTP API 暴露：

- `GET /health`：进程存活。
- `GET /ready`：数据库、catalog 和市场同步可用性。
- `GET /sync-health`：每个 trade hub 的最新同步时间、状态、失败信息和连续失败次数。

`EVETOOLS_DATABASE_URL` 只能配置在服务端 HTTP API、worker、catalog admin CLI 或托管 job 环境中。不要把它写入桌面端 `.env`，也不要打包进 Tauri 应用。桌面端只使用 `EVETOOLS_API_BASE_URL` 访问 hosted API。

同步至少一个 region 后，先启动 hosted HTTP API，再启动桌面端查看快照驱动的 Selection Discovery：

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
export EVETOOLS_HTTP_ADDR="127.0.0.1:8080"
cargo run -p evetools-http-api --bin evetools-http-api
```

另开一个 shell 启动桌面端：

```bash
export EVETOOLS_API_BASE_URL="http://127.0.0.1:8080"
pnpm --dir apps/desktop dev
```

## 只读查询 API

`crates/api` 提供第一版只读应用层 API。它不是 HTTP server，也不直接同步 ESI 数据；它封装 repository 查询，作为 Tauri commands 和未来 hosted API 之间的稳定边界。

当前 API 覆盖：

- catalog 导入状态。
- 按 type id 查询本地化物品信息。
- 按关键字搜索本地化物品信息。
- 查询启用的 trade hubs。
- 查询指定 station 的最新成功市场订单快照。
- 基于启用 trade hubs 的最新成功订单快照生成 Selection Discovery 推荐，并支持按 hub id 过滤。

`crates/http-api` 是第一版 hosted HTTP adapter，复用 `EveToolsReadApi`，提供：

- `GET /health`
- `GET /ready`
- `GET /sync-health`
- `GET /catalog/status`
- `GET /inventory-types/{type_id}?language=zh-CN`
- `GET /inventory-types/search?query=tri&language=en-US&limit=20`
- `GET /market-lookup?query=tri&language=zh-CN&hub_id=jita`
- `GET /trade-hubs`
- `GET /station-orders?region_id=10000002&station_id=60003760&limit=100`
- `GET /selection-candidates?hub_ids=jita,amarr&language=zh-CN&limit_per_hub=25`

桌面端只读取 `EVETOOLS_API_BASE_URL`，不再直接读取 `EVETOOLS_DATABASE_URL`。`EVETOOLS_DATABASE_URL` 只应存在于服务端 HTTP API、worker CLI、catalog admin CLI 或托管 job 环境中。

## 静态 SDE Catalog

EveTools 通过 Rust catalog service 将 CCP 官方 SDE JSON Lines 压缩包导入 Supabase Postgres。

### 数据库连接

运行 catalog 导入、worker 同步或 hosted HTTP API 前，在对应服务端/admin shell 中设置数据库 URL：

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
```

连接串从 Supabase Dashboard 的 `Connect` 面板获取。选择 direct connection 和 pooler 时，可参考 Supabase 的 [database connection guide](https://supabase.com/docs/guides/database/connecting-to-postgres/serverless-drivers)。对于 catalog 导入，优先使用以下连接方式：

- 启用 SSL 的 Direct Postgres connection。这是本地/admin 导入的首选方式。
- 如果本地网络无法访问 direct IPv6 endpoint，可以使用 Supavisor session pooler。
- 不要把 transaction pooler 用于这个 importer。Importer 需要稳定的 session 语义并执行批量写入；transaction pooling 更适合短生命周期/serverless 流量，可能让导入停在 `idle in transaction`。CLI 会拒绝 `*.pooler.supabase.com:6543` 这类 URL。

URL 必须启用 SSL。没有 query 参数时使用 `?sslmode=require`；如果 URL 已有 query 参数，则追加 `&sslmode=require`。如果你已在 Supabase 启用 SSL enforcement，并在本地安装了项目 CA 证书，`sslmode=verify-full` 更强。

需要让 repository integration tests 真实访问 Postgres 时，使用本地一次性测试 URL：

```bash
docker compose -f docker-compose.test.yml up -d
export EVETOOLS_TEST_DATABASE_URL='postgresql://postgres:postgres@127.0.0.1:54329/evetools_test?sslmode=disable'
cargo test -p evetools-db --test catalog_repository -- --nocapture
```

未设置 `EVETOOLS_TEST_DATABASE_URL` 时，Postgres integration tests 会自动跳过。测试启动后会先删除 `evetools_catalog` schema 和 `_sqlx_migrations`，再重新执行 migrations，因此它只适合本地 disposable Postgres。`evetools-test-support` 默认拒绝非本地主机，避免测试样例把完整 SDE 数据替换成一行 fixture。

### SQL Migrations

数据库 schema 使用 SQLx 版本化 migrations，目录位于 `crates/db/migrations`：

- `0001_create_catalog_schema.sql`：SDE catalog 核心表、外键和基础索引。
- `0002_add_catalog_localizations.sql`：标准化多语言 localization 表和语言检索索引。
- `0003_add_market_sync_tables.sql`：trade hub、market sync run 和订单快照表。
- `0004_add_market_sync_operations.sql`：公开市场同步 lease、运行元数据和活跃同步约束。

应用仍通过 `migrate_catalog_schema()` 执行迁移；它会调用内嵌的 SQLx migrator，并在数据库的 `_sqlx_migrations` 表记录已应用版本。新增 schema 变更时不要再把 SQL 追加到 Rust 字符串常量中，应新增一个递增编号的 migration 文件，并补充对应 repository 或 schema 测试。

需要手动初始化或修复完整官方 SDE catalog 时，使用 admin CLI：

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
cargo run -p evetools-catalog --bin import-sde-latest
```

这个 CLI 只是薄入口：读取 `EVETOOLS_DATABASE_URL`，调用 Rust `CatalogService::import_latest()`，并输出导入状态和行数。正式维护时可以把同一套 service 方法接到 hosted job、worker、GitHub Actions 或 Supabase 托管函数上；不要把特权数据库连接串分发给最终用户桌面端。

CLI 会显示阶段级进度：检查 SDE metadata、检查当前 catalog、下载或复用本地缓存、解析后的行数、写入 Postgres 的分表行数，以及最终状态。下载阶段第一版不显示百分比；写库阶段每 1000 行和每张表最后一行报告一次。

Importer 会按 SDE build number 将下载的 zip 暂存到本地缓存，默认目录是系统临时目录下的 `evetools/sde`。也可以用 `EVETOOLS_SDE_CACHE_DIR` 指定目录：

```bash
export EVETOOLS_SDE_CACHE_DIR="$PWD/.cache/sde"
```

数据库已经是完整最新版本时不会下载。导入失败时，缓存文件会保留，下一次重试会直接复用同一 build 的 zip；导入成功并写库完成后，缓存文件会被删除。

如果导入曾经长时间停在某张表的 `0 / total`，先检查连接串是否用了 transaction pooler。已卡住的进程可以用 `Ctrl+C` 中断。当前 importer 会拒绝 transaction pooler，并使用按批次执行的 `COPY -> session temp staging table -> merge` 写入链路；每批独立提交，失败时只重试当前批次，避免整张大表一次性 merge 长时间占用连接。旧 catalog rows 的清理只会在分表写入完成后执行，最后才标记导入成功。

SDE 实体名和描述会导入到标准化 localization 表中。`get_inventory_type` 和 `search_inventory_types` 在服务端根据请求语言选择显示名，前端只传当前语言，不解析 SDE 多语言 JSON。语言 fallback 顺序为精确语言、基础语言、中文 fallback、英文 fallback，再退回任意可用名称。升级到包含 localization 表的版本后，需要重新运行一次完整 SDE 导入来填充这些表。

不要提交真实数据库 URL 或密码。不要把它们放进已纳入版本控制的 `.env` 文件。如果凭据出现在聊天、日志、截图或源码控制中，请先在 Supabase 中轮换后再使用。

Direct Supabase Postgres 模式只适用于本地、私有或 admin catalog 导入、worker 同步、hosted HTTP API 服务端环境。`EVETOOLS_DATABASE_URL` 是特权凭据：不要把它打包进 Tauri app，不要注入给最终用户，也不要要求最终用户持有它。桌面端只配置 `EVETOOLS_API_BASE_URL`。

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
- `crates/api`：只读查询 API，封装 catalog 和 market repository，供 Tauri 与未来 hosted API 复用。
- `crates/http-api`：Axum hosted HTTP adapter，对外暴露只读查询接口。
- `crates/worker`：同步状态和 worker 边界。
- `crates/test-support`：本地 Postgres integration test 保护、schema 重建和远程测试库防误用。

桌面应用位于 `apps/desktop`：

- `apps/desktop/src`：React UI 和 typed Tauri command wrappers。
- `apps/desktop/src-tauri`：Tauri 2 Rust crate 和 command handlers。

React 渲染后端准备好的 views，并调用 Tauri commands。Tauri commands 是 Rust crates 的适配层；交易计算应保留在 `crates/domain` 中。

桌面端查询 catalog、Market Price Lookup、trade hubs 和 Selection Discovery 时通过 `EVETOOLS_API_BASE_URL` 调用 hosted HTTP API；不在桌面进程中持有 Supabase/Postgres 特权连接串。

## MVP 界面

第一个桌面屏幕包含三个界面：

- `Market Price Lookup`：查询某个物品当前 Jita 价格状态。
- `Selection Discovery`：基于最新 hub 快照列出推荐物品、交易点、入场价、出场价、净利润、评分和理由。
- `Order Monitor`：展示类似活跃订单的 fixture rows，并给出建议动作和紧急度。

同步状态分为公开和私有流程：

- Public market sync：`live-ready`、`fixture-ready` 或 `fixture-fallback`
- Authenticated order sync：`not-authorized`
- Data source：`live` 或 `fixture`

## 范围

当前 foundation 范围内：

- 本地 Tauri 桌面壳。
- 基于 hosted API 和 Supabase 快照的 Jita 市场查询。
- 基于 Supabase 市场订单快照的多 hub Selection Discovery。
- Fixture fallback command 边界。
- 连接到 Tauri commands 的 React UI。
- 确定性、可测试的 Rust 领域计算。

当前 foundation 范围外：

- 全区域、全历史市场数据库。
- EVE SSO token 处理。
- 认证角色订单同步。
- 自动下单、改单或撤单。
