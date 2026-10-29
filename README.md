# 淘书工坊 · Taoshu Studio

在桌面里完成「淘宝采集 → 孔夫子找图 → 图片净化 → 闲鱼发布」的二手书工作台。

- **Rust + Tauri 2**：桌面壳、三个独立 WebView2 窗口、本地 HTTP 服务、图片下载。
- **Vue 3 + Vite**：工作台界面（列表、详情抽屉、图片编辑器、设置）。
- **SQLite（rusqlite bundled）**：采集、图书、图片、发布任务和设置保存在本机；启用接口发布时，商品资料发送到闲管家，选择 Gitee 托管时图片上传到指定的公开仓库。

> 这是自用效率工具，不是爬虫平台。请遵守淘宝、孔夫子、闲鱼的平台规则、 robots 协议与著作权要求；账号风控、图片版权和商用授权风险由使用者自行承担。

---

## 1. 功能

### 淘宝采集

- 打开「淘宝」窗口，登录后浏览商品详情页。
- 工具条点「抓取本页」，读取标题、价格、原价、店铺、销量、主图、详情图、规格参数。
- 自动去重入库（同一商品重复抓取会更新旧记录），并按设置计算售价：`ceil(max(成本 × (1 + 目标利润率), 成本 + 最低利润))`，默认利润率 15%、最低利润 5 元。
- 定价规则只影响之后新采集的书；已有书目不会自动改价，避免覆盖手工设定的售价。
- 工作台列表可搜索、按状态筛选、编辑、删除、导出 CSV。

### 单本图书 AI 整理货源

在「图书详情 → 货源」操作已有图书：

- **搜索并用 AI 整理**：选择 OpenCode / Qoder CLI 和模型，打开原生淘宝采集窗口；采集结束后自动整理一次。登录或验证码由你在窗口完成，再继续检索；也可停止并整理已采集候选。本次执行器、模型和提示词在开始搜索时固定。
- **添加链接 / 手动录入**：每次可粘贴最多 20 个含商品 ID 的淘宝 / 天猫完整商品链接，自动去重；短链接需先在浏览器展开。添加链接只保存待核验候选，不会自动抓取详情或确认可用。随后「打开商品采集」补充详情，或手动填写资料，再点「整理已有货源」。
- 比较表展示版本、采购价、运费、代发条件、推荐和备用理由；缺失资料明确显示待核实。资料变化后旧报告会标记「资料已变化」，需要重新整理。
- AI 不会自动设主选。点击「核对并设主选」后，必须补齐规格与采购价，将货源状态设为「可用」，勾选「本次已核验」并保存；运费与代发条件仍须人工核对。
- 「设置 → AI 流程 → 提示词模板 → 整理货源」保存专用提示词；整理会使用所选模型额度。货源报告独立保存，本次整理不会覆盖销售草稿、售价或当前主选。

完整经营流程与验证边界见 [COMMERCE_WORKFLOW.md](COMMERCE_WORKFLOW.md)。

### 孔夫子找图

- 列表项点「找图」，按 ISBN（没有则用书名）打开孔夫子搜索页。
- 工具条点「选图导入」，扫描页面里较大的封面图，弹窗多选后下载到本地。
- 下载时会带 Referer、校验文件头、限制 20MB，失败会逐条回报，不会中断整批。

### 图片净化

- 图片编辑器中提供：画笔涂抹、橡皮、自动浅色水印检测、扩散修补（inpaint）、裁剪、白底增强。
- 导出 PNG 覆盖「处理版」，发布时优先使用处理版，原图保留以便回溯。
- 可设主图、删除图片、上传本地图片。

### 闲鱼发布

- 在图书详情里点「发布」，工作台生成发布任务并打开 `https://seller.goofish.com/?site=COMMONPRO#/seller-item/publish` 独立窗口；也兼容卖家中心首页，会自动查找「发闲置」入口。
- 注入脚本读取任务，填写价格、描述，上传图片；默认自动点提交。
- 发布成功后闲鱼会跳转到 `#/seller-item/publish/success` 结果页，脚本会识别该页面并回传 `published`；即使结果页导致整页刷新也能恢复任务。
- 如果出现「快速进入 / 扫码登录」页面，需要先在页面里完成登录，再点工具条上的「继续填表」。
- 新版闲鱼网页发布页没有独立「标题」输入框，脚本会把标题放在描述首行。旧版页面如果存在标题框，仍会分别填写标题和描述。
- 闲鱼会根据图片/描述智能识别分类。书籍常被识别为「教材教辅」，但网页版不支持该分类并会要求扫码去 APP 发布；脚本检测到后会**自动切换为「其他闲置」**，让网页发布按钮可用。这个分类可能影响曝光；不接受时请关闭自动提交，手动改回分类并在 APP 扫码发布。
- **自动填表是启发式的**：闲鱼前端改版后选择器可能失效。失败时会切换到「手动复制模式」，可以直接复制标题/价格/描述，并保留「再试一次自动填表」按钮。
- 可在设置里关闭自动提交，只填表、由你人工检查后发布。
- 任务状态：`filling`（填表中/待确认）、`published`（已回传成功）、`failed`（需人工处理）。

### 闲鱼工作台（闲管家 API）

- 顶部可切换「本地图书库」「闲鱼工作台」，原有网页发布继续可用。
- 「接入设置」填写已上线的自研进销存应用 AppKey / AppSecret，保存后测试连接；应用及店铺需有闲管家开放平台权益。
- 提供店铺权益与授权到期信息、商品分页与状态/修改时间筛选、详情、文案/单规格价格编辑、库存调整、上下架，以及订单查询和物流发货。
- 商品列表覆盖当前应用范围；订单可按店铺筛选。列表最多查询一万条、最近六个月内的数据。「筛选当前页」仅过滤当前页；可以直接输入闲管家商品 ID 或闲鱼订单号查询详情。
- 多规格商品可查看、编辑文案和上下架；价格与库存请在闲管家管理。首期不含聊天、售后退款和卡密管理，不自动根据订单扣减本地库存。

#### Gitee 图片托管

1. 准备已初始化、允许匿名读取的**公开仓库**，以及对该仓库具有写入权限的 Gitee 私人令牌。
2. 在「闲鱼工作台 → 接入设置 → Gitee 图片托管」填写仓库首页链接、Token、可选分支和图片目录。分支留空使用仓库默认分支。
3. 保存并测试仓库连接。测试只读取仓库；上传权限和匿名图片访问在发布时验证。
4. 从书库点「接口发布」或在工作台点「发布本地图书」。配置 Gitee 后默认选择「Gitee 自动上传」，填好店铺、图书类目、必选属性、发货地区、运费和文案。
5. 图片优先使用净化版，主图优先，最多 9 张。默认「先只测试首图」「只创建」，可取消勾选以发布完整图片并继续上架。
6. 后端将图片以 Base64 上传到 Gitee 文件接口，路径为「图片目录/书目ID/内容摘要.扩展名」，相同内容复用文件。查询返回 HTTP 200 + 空数组也视为文件不存在，会实际上传。优先使用接口返回的下载地址，再不带 Token、Cookie 或 Referer 验证图片内容，才提交闲管家；新图片直链暂时返回 404 时会短暂重试。
7. 若返回登录页、访问拦截或图片不一致，则停止发布并显示原因。匿名校验通过仍不代表闲管家一定能拉取，最终以远端上架和闲鱼页面显示为准。

Gitee Token 和 AppSecret 仅保存在本机 SQLite，不通过配置读取接口回显，也不会附加到图片直链。上传图片公开可见；删除本地图书或图片不会自动删除仓库文件。

#### Base64 实验与公网 URL

- 另外保留「Base64 Data URL」「纯 Base64」「公网 URL」。Base64 属于待验证的兼容性实验，闲管家文档正式要求仍为公网 HTTP/HTTPS 图片 URL。
- Data URL 使用图片实际 MIME；仅在收到明确的图片参数拒绝时自动尝试纯 Base64。超时、非 JSON 或 HTTP 异常不会自动重试创建。
- 「公网 URL」可手动填写最多 9 个图片直链；本机 localhost 地址不能供闲管家拉取。

#### 省市区数据维护

创建商品使用省、市、区三级联动选择，保留商家地址回填。切换上级地区会清空下级选择，前后端均校验地区所属关系；无需手工查编码。

当前数据来自提供的「闲管家省市区-20261019.sql」：34 个省级地区、372 个城市、3639 个区县。原始文件保存在 resources 目录，前端与 Rust 后端共用生成的 src-tauri/resources/xianyu-regions.json，离线可用。

更新时将新 SQL 文件放入 resources（文件名需带 YYYYMMDD 日期），执行：

```powershell
npm run regions:import -- "resources/闲管家省市区-YYYYMMDD.sql"
npm run test:regions
```

导入脚本只解析地区 INSERT 数据，不执行 SQL。更新后重新构建前端和 Rust，以便两端使用同一份地区表。

#### 发布记录与恢复

创建后先保存管家商品 ID，再按选择调用上架接口。返回成功只表示受理，任务保持「上架处理中」；记录页每 15 秒查询最多 5 个处理中任务，也支持手动刷新。

创建超时或应用退出后显示「结果未知」，先点「刷新 / 核查」。按唯一商家编码 taoshu-书目ID-任务ID 查询并恢复关联，不盲目重复创建。已有商品可从记录中继续上架；同一书目、应用、店铺的活动任务有数据库唯一约束。

只有远端确认对应店铺已上架并取得闲鱼商品 ID，才标记「已上架」并更新本地图书状态。打开闲鱼商品页面确认图片正常后，再点「已核对页面，图片正常」记录人工验收。首期不依赖公网回调服务，应用关闭期间不会接收推送。

接口参考：[闲管家开放平台](https://s.apifox.cn/3ac13d69-5a38-4536-ae9b-a54001854ef8/llms.txt)、[Gitee 文件接口](https://gitee.com/api/v5/swagger#/postV5ReposOwnerRepoContentsPath)。

### 三个独立站点窗口

- 「采集」「找图」「发布」分别打开独立 WebView2 窗口，登录态共享，互不打断。
- 每个窗口底部有浮动工具条：地址栏、前进/后退/刷新、站点动作、回工作台、收起。
- 工具条可收起成右下角小胶囊，避免遮挡页面按钮。
- 淘宝、孔夫子大量链接使用 `target=_blank` / `window.open`；WebView2 默认不弹出新窗口，注入脚本会把这类链接改为当前窗口跳转，避免点击“没反应”。

---

## 2. 环境要求

- Windows 10/11（WebView2 运行时；Win11 一般自带）
- Node.js ≥ 20（开发时使用 24.x）
- Rust stable（MSVC 工具链）+ Visual Studio 2022 C++ 生成工具
- 首次构建会下载较多 crate，耗时取决于网络

## 3. 启动

```powershell
npm install
npm run app:dev
```

生产构建：

```powershell
npm run app:build
```

安装包输出在 `src-tauri/target/release/bundle/`（NSIS / MSI）。构建前会自动执行 `npm run build` 生成 `dist/`。

只启动本地 API（调试用）：

```powershell
npm run api:serve
```

默认监听 `http://127.0.0.1:8899`。可用环境变量覆盖：

```powershell
$env:TAOSHU_DATA = "E:\taoshu-data"
$env:PORT = "8901"
npm run api:serve
```

## 4. 数据位置

Windows 默认目录：

```
%APPDATA%\com.taoshu.studio\
  shelf.db        SQLite 数据库
  images\         原图 / 处理图
  .token          工作台 API token（本机）
```

- 备份：退出应用后复制整个目录即可。
- 重置：退出应用后删除 `shelf.db`（会清空书库；图片文件可另行保留）。
- 图片接口是公开只读的（按 id 访问，便于 `<img>` 显示）；写接口需要 token 或注入密钥。

## 5. 使用流程

1. **设置**：工作台右上角「设置」里确认目标利润率（建议 10%～20%）、最低利润（默认 5 元）、默认成色、标题后缀、描述模板、发布地址、孔夫子搜索地址、是否自动提交。
2. **采集**：点「淘宝采集」→ 在独立窗口登录淘宝 → 打开商品详情页 → 工具条「抓取本页」→ 回工作台确认。
3. **找图**：列表点「找图」→ 孔夫子页面点「选图导入」→ 勾选封面 → 导入。
4. **净化**：详情里点图片「净化」→ 涂抹/自动检测/修补/裁剪 → 保存。
5. **发布**：详情点「发布」→ 闲鱼窗口登录 → 自动填表（或手动复制）→ 检查后提交。
6. **回传**：脚本提交后会调用 `/api/listings/{id}/report` 更新状态；不确定时标记 `filling`，可在工作台看到提示。

> 闲鱼建议直接使用商品发布页 `https://seller.goofish.com/?site=COMMONPRO#/seller-item/publish`。如果登录后被跳到其他域名，工具条仍会出现，但自动填表只对当前页面做启发式匹配。

## 6. Chrome 扩展兜底（可选）

当 WebView2 内登录/发布受限制，或你更习惯 Chrome，可以用扩展把同一套注入脚本加载到浏览器：

```powershell
npm run extension
```

然后在 Chrome：

1. 打开 `chrome://extensions`，开启「开发者模式」。
2. 「加载已解压的扩展程序」→ 选择项目里的 `extension` 目录。
3. 先启动淘书工坊桌面端（提供本地 API），再刷新淘宝/孔夫子/闲鱼页面。

扩展后台通过 `/api/token` 拿工作台 token，再代理 API 与图片请求；内容脚本不读取页面 JS 变量。扩展不包含账号密码，也不上传数据到第三方。

## 7. 本地 API 摘要

| 方法 | 路径 | 鉴权 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/health` | 无 | 健康检查 |
| GET | `/api/token` | Origin 白名单 | 工作台 token |
| POST | `/api/captures` | token/secret | 入库（会创建/更新图书） |
| GET | `/api/books` | token | 图书列表（`q`、`status`、`limit`、`offset`） |
| GET | `/api/books/{id}` | token | 图书详情（含图片、发布记录） |
| PATCH | `/api/books/{id}` | token | 编辑图书/发布文案 |
| DELETE | `/api/books/{id}` | token | 删除图书与图片文件 |
| POST | `/api/books/{id}/upload` | token/secret | 上传图片（`Content-Type: image/*`） |
| POST | `/api/books/{id}/images` | token/secret | 按 URL 批量下载图片 |
| POST | `/api/images/{id}/processed` | token | 保存处理后的 PNG |
| POST | `/api/images/{id}/primary` | token | 设为主图 |
| DELETE | `/api/images/{id}` | token | 删除图片 |
| GET | `/api/images/{id}/file` | 无 | 原图/处理图（带 CORS） |
| POST | `/api/books/{id}/listing` | token | 创建发布任务与文案 |
| GET | `/api/listings/{id}/payload` | token/secret | 注入脚本读取发布载荷 |
| POST | `/api/listings/{id}/report` | token/secret | 回传发布结果 |
| GET | `/api/export.csv` | token | 导出 CSV |

注入脚本优先走 Tauri IPC（避开 HTTPS 页面访问 localhost 的混合内容/私网限制），失败再回退 HTTP。HTTP 写接口使用启动时随机生成的 `x-taoshu-secret`；工作台使用 `x-kf-token`。

### 闲鱼工作台本地接口

以下接口均使用现有本地 token 鉴权，远端请求由 Rust 后端发出。

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET / PUT | /api/xianyu/config | 读取脱敏配置 / 保存应用凭据 |
| GET / PUT | /api/xianyu/gitee/config | 读取脱敏配置 / 保存图片仓库配置 |
| POST | /api/xianyu/gitee/test | 只读验证公开仓库 |
| POST | /api/xianyu/call/{operation} | 白名单：shops、addresses、categories、properties、products、product、edit、stock、publish、unpublish、orders、order、ship、companies |
| GET | /api/xianyu/books/{id}/preview | 生成图书发布文案 |
| POST | /api/xianyu/books/{id}/publish | 准备图片、创建商品并按需上架 |
| GET | /api/xianyu/tasks | 当前应用最近 100 条接口发布记录 |
| POST | /api/xianyu/tasks/{id}/refresh | 查询结果 / 核查未知创建 |
| POST | /api/xianyu/tasks/{id}/publish | 核查后继续上架已有商品 |
| POST | /api/xianyu/tasks/{id}/verify-images | 标记已在闲鱼页面人工核验图片 |

远端商品、订单和授权 ID 在前端使用字符串，Rust 按文档传入整数 ID。call 接口金额为整数分，图书 publish 表单金额为元。

## 8. 常见问题

**端口被占用 / 本地服务未连接**

- 关闭残留的 `taoshu-studio.exe` / `api-server.exe`。
- 桌面端固定使用 `http://127.0.0.1:8899`；如需改端口，必须同时修改前端与注入脚本里的 API 地址。
- 桌面端启动时会打印实际监听地址；工具条左上角圆点变绿表示已连接。

**抓取不到**

- 确认是商品详情页（URL 含 `id=` 或 `/i<数字>.htm`），不是搜索页/店铺页。
- 页面刚打开时等图片和规格渲染完再点。
- 淘宝改版会导致选择器失效，可在 `src-tauri/inject/browser.js` 的 `extractTaobao` 里补充选择器。

**孔夫子选图列表为空**

- 孔夫子是 Vue SPA，脚本只能读取当前 DOM；先让搜索结果滚动加载出封面。
- 图片过小（小于 120px）会被过滤，避免把图标/二维码导进来。

**闲鱼自动填表失败**

- 工具条面板会显示失败原因并进入手动复制模式；复制标题/价格/描述后手动发布。
- 首次使用需要先完成登录，再点「再试一次自动填表」。
- 页面提示「网页版暂不支持发布此分类，请使用闲鱼 APP 扫码」时，脚本会自动尝试改为「其他闲置」；如果仍失败，只能手动选分类或在 APP 扫码发布。
- 闲鱼前端是 React 受控组件，描述框使用 `execCommand` 写入并触发输入事件；价格使用原生 setter + `input/change`。若页面改版，需要更新 `findPublishForm` / `ensureCategorySupported` / `findSubmitButton`。

**图片净化后发布仍用原图**

- 发布载荷优先使用 `processed`；保存处理图后刷新图书详情确认。
- 若处理失败，详情里会保留原图；可重新进入编辑器。

**构建报错找不到 esbuild**

- 本项目使用 Vite 8（rolldown/oxc），不要再给 `build.minify` 配 `esbuild`。

## 9. 风险与合规提示

- **平台规则**：自动化抓取、自动发布可能违反淘宝/闲鱼用户协议，可能导致限流、验证码、封号。建议降低频率、只处理自己有权售卖的商品。
- **著作权**：孔夫子上的书籍封面、实拍图可能受著作权保护。去水印后用于商业发布尤其需要授权；请优先使用自己拍摄的图片，或取得权利人许可。
- **个人信息**：不要采集/发布他人的个人信息、联系方式或盗版商品。
- **账号安全**：登录在 WebView2/Chrome 内完成，本工具不保存账号密码；但本机 token 文件可被同机进程读取，请勿在多人共用的电脑上使用。
- **自动提交**：默认开启仅为省事；建议首次关闭自动提交，确认表单填充正确后再开启。

## 10. 目录结构

```
src/                      Vue 工作台
  components/             顶栏、列表、详情抽屉、图片编辑器、设置、Toast
  api.js                  前端 API 封装与窗口打开
  App.vue                 主流程、轮询、发布
src-tauri/
  src/
    api.rs                axum 路由与鉴权
    db.rs                 SQLite 表结构与查询
    images.rs             图片下载、校验、保存
    xianyu.rs             闲管家客户端、操作白名单、发布记录与恢复
    gitee.rs              图片仓库配置、去重上传、匿名直链验证
    listing.rs            发布文案生成
    lib.rs                Tauri 启动、窗口、IPC 桥
    state.rs / models.rs  状态与数据模型
    bin/api-server.rs     独立 API 服务
  inject/browser.js       三站点注入脚本（工具条/采集/找图/发布）
  capabilities/           主窗口与远程窗口权限
  tauri.conf.json         窗口、CSP、打包配置
extension/                Chrome 兜底扩展（manifest + 后台）
scripts/
  build-extension.mjs     生成扩展内容脚本
  make-icon.mjs           生成图标源图
  smoke.mjs               API 冒烟测试
```

## 11. 开发检查

```powershell
npm run build                                   # 前端构建
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml --lib # 隔离的闲鱼/Gitee 模拟测试
npm run extension                               # 重新生成扩展脚本
```

单本货源隔离验收（先构建 API，需要 Rust 工具链）：

```powershell
cargo build --manifest-path src-tauri/Cargo.toml --bin api-server --offline
node scripts/source-ai-smoke.mjs
```

脚本在临时数据目录启动独立 API，并用 `rustc` 编译原生 fixture CLI；模型回答与淘宝结果均为模拟，不调用真实模型或验证真实淘宝采集。覆盖链接去重、采集补充、货源比较、过期报告保护、搜索后自动整理，以及不自动设主选或覆盖销售草稿。

API 冒烟（另开一个终端）：

```powershell
$env:TAOSHU_DATA = "E:\workspace\git\kong\.smoke-data"
$env:PORT = "8901"
npm run api:serve
# 再开一个终端
$env:BASE = "http://127.0.0.1:8901"
$env:DATA = "E:\workspace\git\kong\.smoke-data"
node scripts/smoke.mjs
```

修改 `src-tauri/inject/browser.js` 后必须重新编译 Rust（`include_str!` 会在编译期嵌入脚本）；扩展模式则重新运行 `npm run extension` 并在 `chrome://extensions` 点刷新。


### OpenCode 与 Qoder CLI

- 在「设置 → Agent 接入」切换 OpenCode / Qoder CLI，可选择本机程序或安装应用专用版。专用版保存在应用数据目录的 `runtime/` 内，不修改系统 PATH。
- 两者共用固定项目目录 `<应用数据目录>/agent-support/workspace`，设置页面可查看完整路径。所有应用发起的 CLI 任务、版本和模型检测均显式设置工作目录，AI 草稿附件与日志保存在该目录内部。
- OpenCode 保留内嵌网页，网关将 API 请求绑定到固定目录并拒绝通过查询参数或请求头切换目录。Qoder 使用原生交互终端，启动时同时指定进程工作目录与 `--cwd`；关闭终端或退出应用会清理其进程树。
- 开启 MCP 导入后会生成两个 Agent 的项目配置与 `taoshu-import`、`taoshu-workflow` Skills。Qoder 使用项目 `.mcp.json` 和 `.qoder/skills/`，通过 `--strict-mcp-config` 加载项目指定的 MCP；首次启动按终端提示登录。
- Qoder 专用版采用[官方安装脚本](https://docs.qoder.com/zh/cli/installation)引用的原生发行源，校验官方 SHA-256 后提取程序；[CLI 参数文档](https://docs.qoder.com/zh/cli/cli-reference)说明 `--cwd` 等选项。
- 「AI 流程」可选择 OpenCode 或 Qoder CLI，并分别保存模型；单本或批量“AI 完善”可覆盖本次执行器和模型。自动任务使用保存的默认值，任务创建后冻结执行器和模型，暂停、继续及重试均沿用原配置。复制命令和 MCP `taoshu_generate_draft` 同样支持 `provider: "opencode" | "qoder"`、`model`。
- Qoder 登录状态由 `status -o json` 查询，界面只返回是否登录，不暴露账户标识或凭据。“登录 Qoder”在固定目录打开官方 `login` 流程，无需先开启 MCP；完成后自动刷新登录状态与模型。交互终端使用 AI 流程中保存的 Qoder 模型，重新打开后生效。
- 两个执行器生成自动草稿时均禁用工具；Qoder 使用非交互 JSON 输出、标准输入传递图书证据，并禁用 MCP 与 Hooks。此处固定的是应用启动的项目目录，并非操作系统沙箱；交互终端的文件访问仍由各 CLI 的权限规则管理。

验证：`node --test scripts/opencode-web.test.mjs`；`node scripts/qoder-smoke.mjs --install` 会在隔离数据目录中验证真实下载安装及模拟终端生命周期，不发送模型请求。已有后端运行时可通过 `TAOSHU_TEST_API_BINARY` 和 `TAOSHU_TEST_PORT` 指定独立测试程序与端口。

`node scripts/qoder-ai-smoke.mjs` 使用明确模拟的 Qoder CLI 验证登录、模型列表、长提示词、真实队列与草稿写入、暂停后切换默认执行器、失败重试和 MCP 命令行，不调用远程模型。真实账户的浏览器登录由用户完成；真实 Qoder 1.1.80 已验证登录状态查询及非交互启动参数兼容性。

### 订单关联本地图书

- 订单列表按当前应用的发布记录匹配管家商品 ID、闲鱼商品 ID 或商家编码；多个匹配发生冲突时保持未关联，不按书名猜测。
- 打开订单详情会重新向闲管家查询商品库存，同时显示本地图书库存和本单数量；查询失败时保留订单详情并显示原因。这里只查看库存，不会扣减或覆盖本地库存。
- 未匹配的订单可按书名、ISBN、作者搜索本地图书后保存关联；可解除关联，手动选择会保留到下次刷新。
- 本地图书详情的“关联闲鱼订单”显示已查询并关联的订单，可跳转到工作台重新查询实时详情。订单历史不是全量同步结果，尚未查询的订单需要先在工作台查询。
- 本地只保存订单号、状态、商品、数量、金额和时间等历史摘要，不保存买家收件资料；关联按 AppKey 隔离。
