# Single-book source comparison — review packet

Request: 用户先完善单点功能，已有一本图书，希望通过 AI 整理货源。已确认：两者都要，先把单本书流程跑通（搜索货源，以及整理自行提供/已采集的链接）。

Direction: extend the existing Operate surface in 图书详情 → 货源. Inherit PRODUCT.md and DESIGN.md: light, compact Chinese lists, existing teal controls, 1360×880 and minimum 1080×700. No new top-level navigation or visual identity. Search and pasted links join existing book_sources; source AI reports remain separate from sale drafts. Unknown data is explicit, comparisons retain product links, recommendations are checked manually before selecting a primary source. Search uses the existing logged-in Tauri Taobao collector; browser/model results in acceptance are simulated.

Artifact: src/components/redesign/BookSourceAi.vue, integrated into src/components/BookOperations.vue, with src/redesign.css and source mode in AiGenerateDialog.vue. Backend: agents/source_analysis.rs, agents/ai.rs, commerce/source_links.rs, commerce/price_search.rs. New source reports cover all candidates, reject unknown IDs and invalid recommendations, detect changed evidence, preserve prior reports on failure, and preserve sale drafts/prices. Primary selection requires status=available plus existing SKU/price/check validation.

Required captures in .impeccable/review/: source-ai-desktop.png (1360×880), source-ai-minimum.png (1080×700), source-ai-mobile.png (390×844); source-ai-confirm.png, source-ai-empty.png, source-ai-link-error.png, source-ai-searching.png. All are full-page shots; motion is disabled for final captures. Mobile uses a keyboard-focusable horizontally scrollable comparison region with an explicit scroll hint.

Quality bar: task actions visible, recommendation reasons paired with evidence and missing fields, no invented verification, no whole-page overflow, existing design system preserved. No approved comp: local extension built directly from incumbent system. Craft floor: .agents/skills/impeccable/reference/craft-floor.md.

Verification: Rust tests, source-ai-smoke.mjs (isolated API + native fixture CLI), real Vue UI through Playwright with isolated API. Fake model and marketplace results; no real model billing or transactions. UI checks direct analysis, search creation, manual primary dialog and Escape, empty candidates, invalid links and valid retry, desktop/minimum/mobile overflow.

Detector: /tmp/busy-book-source-detect.json. One primary finding is pre-existing font-family: Consolas at redesign.css:209 (documented in DESIGN.md prose but absent from frontmatter typography). 52 advisories across existing stylesheet; new 12px text matches DESIGN.md prose. No design-system repair requested. Detector ran once successfully on changed UI targets; initial --target invocation was an unsupported usage and scanned nothing.
