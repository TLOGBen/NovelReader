# Tasks: shared
**前置群組**：無

## TASK-shared-01: 建立 utils/ 骨架 + 搬出 resolve() helper

**需求追溯**：REQ-001 (utils/ 須存在), REQ-005 (編譯品質不退化)
**目標**：`src/utils/` 目錄存在，至少包含一個 helper（resolve URL），main.rs 已宣告 mod；其餘程式碼透過 `crate::utils::url::resolve` 引用，原 `scraper.rs::resolve` 移除。

**驗收標準**：
- [ ] `src/utils/mod.rs` 與 `src/utils/url.rs` 存在
- [ ] `src/utils/url.rs` 包含從原 `src/scraper.rs` 搬出的 `pub fn resolve(base: &str, href: &str) -> Result<String>`
- [ ] `src/scraper.rs` 內 `resolve` 私有函數已移除，呼叫處改用 `crate::utils::url::resolve`
- [ ] `cargo build --bin novel-looker`（含 `LIBCLANG_PATH`）通過，無新 warning
- [ ] `cargo test` 全綠

### 步驟

#### 建立 utils 模組
- [ ] `mkdir -p src/utils/`
- [ ] 建立 `src/utils/mod.rs`，內容只含 `pub mod url;`
- [ ] 建立 `src/utils/url.rs`，從 `src/scraper.rs` 複製 `resolve` 函數體（含 `use anyhow::{Context, Result};` 與 `use url::Url;`），改為 `pub fn resolve(...)`

#### 更新引用
- [ ] `src/main.rs` 加上 `mod utils;`（在 alphabetical 順序適當位置）
- [ ] `src/scraper.rs` 刪除原 `fn resolve(...)` 定義
- [ ] `src/scraper.rs` 把**全部** `resolve(...)` 呼叫（grep 應為 5 處 call site + 1 處 fn 定義）改為 `crate::utils::url::resolve(...)`，原 fn 定義刪除

#### 驗證
- [ ] `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker`
- [ ] `cargo test`
- [ ] `cargo run -- search "alice"`（Gutenberg 走 URL resolve 路徑）回傳 ≥ 5 筆結果
