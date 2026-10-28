---
name: 淘书工坊
description: 简洁、紧凑的图书经营界面
colors:
  paper: "#f5f7f8"
  paper-2: "#edf1f3"
  card: "#ffffff"
  ink: "#202c35"
  ink-2: "#354650"
  muted: "#657581"
  line: "#dfe5e9"
  line-strong: "#bbc8ce"
  accent: "#137d72"
  accent-soft: "#e9f4f1"
  danger: "#b03f43"
  success-text: "#287451"
  success-bg: "#e8f3ee"
  warning-text: "#95621d"
  warning-bg: "#fcf1dd"
typography:
  body:
    fontFamily: 'Segoe UI, Microsoft YaHei, sans-serif'
    fontSize: '14px'
    lineHeight: 1.5
  title:
    fontFamily: 'Segoe UI, Microsoft YaHei, sans-serif'
    fontSize: '20px'
    fontWeight: 650
    letterSpacing: '0'
  label:
    fontFamily: 'Segoe UI, Microsoft YaHei, sans-serif'
    fontSize: '13px'
    fontWeight: 550
rounded:
  control: '5px'
  surface: '6px'
  modal: '8px'
spacing:
  compact: '8px'
  field: '12px'
  group: '16px'
  section: '24px'
  page-inline: '30px'
components:
  button-primary:
    backgroundColor: '{colors.accent}'
    textColor: '{colors.card}'
    rounded: '{rounded.control}'
    padding: '7px 13px'
  button-secondary:
    backgroundColor: '{colors.card}'
    textColor: '{colors.ink}'
    rounded: '{rounded.control}'
    padding: '7px 13px'
  input:
    backgroundColor: '{colors.card}'
    textColor: '{colors.ink}'
    rounded: '{rounded.control}'
    padding: '8px 10px'
  nav-active:
    backgroundColor: '{colors.card}'
    textColor: '{colors.accent}'
    rounded: '{rounded.surface}'
    padding: '12px 17px'
  data-surface:
    backgroundColor: '{colors.card}'
    rounded: '{rounded.surface}'
---

# Design System: 淘书工坊

## Overview

**Creative North Star: “图书经营台”**。以中文数据列表、明确操作和短状态标签构成日常经营界面。浅色背景、白色内容区和克制的青绿色操作色，服务高频桌面操作。

样式依据为 `src/redesign.css`，最后加载以覆盖保留的业务组件样式。用户已批准重新设计全部页面组织；旧页面仅作为业务能力参考。

## Colors

中性灰白区分页面、内容与边界。accent 用于主操作和选中状态，danger 用于危险操作；成功、警告和失败标签使用各自的语义色。长文本用 ink，辅助文字用 muted。颜色必须配合文字状态，不能作为唯一信息载体。

## Typography

采用 Segoe UI、Microsoft YaHei 和系统 sans-serif。正文 14px，控件与列表主要文字 13px，辅助文字 12px，页面标题 20px。金额和数量使用等宽数字；Consolas 仅用于编号等数据。

## Layout

四个一级入口：闲管家、图书、采集、设置。侧栏 172px、顶栏 78px，内容区左右 30px，上方 24px，最大宽度 1700px。默认桌面 1360×880，最低验收窗口 1080×700。

1150px 以下侧栏收窄为 144px；800px 以下提供图标侧栏与单列降级。表格使用紧凑行、列标题和必要滚动，筛选紧邻列表。详情与列表共享数据身份，返回保留查询与位置。

## Elevation & Depth

普通内容区用 1px 边框分隔，不用大面积阴影。弹窗使用 `0 12px 48px #1327311a`；选中侧栏项仅有 `0 1px 3px #2338430a`。浮层高度限制在视口内，内部允许滚动。

## Shapes

控件圆角 5px，普通内容面 6px，弹窗 8px。图标来自 Lucide。状态标签保持紧凑，避免用大卡片放大无关信息。

## Components

按钮最小高度 34px，主要按钮青绿底白字，次要按钮白底边框，辅助操作用透明按钮。输入最小高度 36px，聚焦边框使用 accent；键盘焦点为 2px accent 描边，外偏移 3px。

表格负责比较和批量处理，页签切换同一模块内的视角。图书详情固定资料、货源、图片、销售记录。帮助、证据、电子资源和核算按需展开，正常页面不放大段说明。

弹窗保持焦点循环并在关闭后还原焦点，原生 summary 参与键盘导航。选择菜单先响应 Escape，Tab 收起菜单并返回触发控件。采集连接错误与操作失败分开显示，恢复连接仅清除过期连接错误。

禁用操作保留可识别状态，必要时说明不可执行原因；未验证的平台能力不因界面改版开放。尊重 prefers-reduced-motion。

## Do's and Don'ts

- Do 使用短字段名、状态文字和明确动作名称。
- Do 将货源与图片操作放入对应图书详情。
- Do 保持本地库存与渠道可售数量独立。
- Do 就地展示失败原因，并保留可恢复的操作上下文。
- Don't 新增工作台、独立货源中心或图片中心。
- Don't 用装饰统计卡、口号或长介绍占据业务页面。
- Don't 把等待人工确认或未知平台结果标记为成功。
