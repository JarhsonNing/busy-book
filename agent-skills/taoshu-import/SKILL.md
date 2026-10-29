---
name: taoshu-import
description: 通过淘书工坊 MCP 查询 ISBN、单本或批量入库图书，并导入学校专业用书、货源和图片。适用于 OpenCode 或其他本地 Agent 整理公开书目、补充货源和图片；不处理采购支付、闲鱼发布或发货。
---

# 淘书工坊资料导入

连接用户在“设置 → Agent 接入”中提供的 `taoshu` MCP。应用须处于运行状态。先调用 `taoshu_context` 获取工具与字段约定，再按实际需求检索或导入。

## 导入约定

- 使用真实公开来源。为书目和学校用书保留 `sourceUrl`，不要把模型生成的出版社、ISBN、版次或运费当成检索事实。
- `source` 表示本次数据来源系统，`externalId` 是该系统中稳定的记录编号。重试沿用这两个值及原参数；工具拒绝用同一编号覆盖不同内容，返回已保存结果时直接复用。
- 先用 `taoshu_search_books` 查询已有版本。ISBN 相同仍须核对版次与册次；冲突时将候选交给用户选择，不能仅按书名合并。
- 缺 ISBN 时优先调用 `taoshu_lookup_isbn({query:书名})`。它查询豆瓣建议列表，并尝试提取一个书名完全匹配候选的详情；其他候选用返回的数字 `subjectId` 再查。对照作者、出版社、版次和册次选定版本，不能把第一条或同名候选的 ISBN 直接应用到图书。`blocked`、`failed`、`detailError` 和 `isbn_missing` 均不能视为已取得 ISBN；遇到安全验证停止请求，尊重 `retryAfterSeconds`，不换通道绕过限制。查询自带缓存，不必反复请求，也不会自动改书库。
- 单本用 `taoshu_import_book`；多本用 `taoshu_import_books({books:[...]})`，每批 1–200 条，每条字段与单本相同，分别提供真实 `source`、稳定 `externalId`、`sourceUrl`、书名和有依据的版次，ISBN 等未知文本字段省略或留空。资料不足的条目保留失败原因，不编造版次来通过校验。查看 `items` 中逐条的 `created`、`matched`、`reused`、`failed` 和 `bookId`，不能只看请求成功就宣布全部入库。失败条目可修正后重试；已成功条目沿用原参数可安全复用。
- 导入返回的 `bookId` 用于后续学校用书、货源和图片关联。
- 货源由 `taoshu_import_source` 保存为本书候选，不覆盖已核验报价。未知采购价、运费填 `null`，不要按零处理。
- 图片使用 `taoshu_import_image` 发送真实图片的 Base64 和公开来源地址。服务不读取 Agent 的本地文件路径，不代抓远程图片。图片应对应当前版本；传入 `mimeType` 为 PNG、JPEG 或 WebP。
- 学校、年份、专业与用书关系使用 `taoshu_import_school_book`，指定已核对版本的 `bookId` 和官方目录 `sourceUrl`。原始引用保留在 `raw`。

## AI 草稿

用户要求定价分析、上架文案或自定义文本时，可调用 `taoshu_generate_draft`：`{action:"start",bookIds:[bookId],task:"complete"}` 创建任务，`{action:"status",jobId:返回编号}` 查询。`pricing` 只定价，`copy` 只生成文案，`text` 生成自定义文本；可传 `prompt`，否则沿用已保存模板。最多 100 本，使用用户已配置的模型，可能消耗模型额度。逐本检查任务结果，不把创建成功当成生成完成。

参考成本由程序取淘宝商品价去重后的第二低价，不含运费；报价不足则待补充，不能让模型猜成本。结果自动写入草稿，不确认主选货源、不发布；模型额度受限时保留失败原因，不自动切换付费模型。仅入库请求不需要额外发起模型调用；应用开启自动流程时由应用处理。

## 完成与失败

以工具返回的 `bookId`、记录编号和 `reused` 为准报告实际入库结果；不要把建议或模型回答描述为已导入。连接失败时提示用户开启应用及 MCP；鉴权失败时使用设置页重新生成的接入配置。不要请求闲管家密钥、Cookie、账号密码、买家资料或业务数据库路径。
