import test from 'node:test';
import assert from 'node:assert/strict';
import {
  extractBookSections,
  htmlToText,
  parseCitations,
  parseLooseCitation,
  parseMajorLabel,
} from './lib/extract.mjs';

test('parses title followed by editor and publisher', () => {
  const [book] = parseCitations('1.《古代汉语（第一、二册）》，王力主编（中华书局，2018年）；');
  assert.equal(book.title, '古代汉语');
  assert.equal(book.volume, '第一、二册');
  assert.equal(book.authors, '王力');
  assert.equal(book.publisher, '中华书局');
  assert.equal(book.year, '2018');
});

test('splits dotted multi-citation and keeps the continuation publisher', () => {
  const text = [
    '初试考试范围',
    '829 数据工程基础',
    '【参考教材】王珊, 萨师煊.数据库系统概论（第五版）.高等教育出版社，2014（第1-11章）',
    '【参考教材】茆诗松，程依明，濮晓龙.概率论与数理统计教程（第三版）.高等教育出版社,2019（第1-8章）李航.统计学习方法（第2版）.',
    '清华大学出版社，2019（第1-5, 7, 9, 19章）',
  ].join('\n');
  const sections = extractBookSections(text);
  const titles = sections.map((section) => section.citation.title);
  assert.deepEqual(titles, ['数据库系统概论', '概率论与数理统计教程', '统计学习方法']);
  const last = sections[2].citation;
  assert.equal(last.authors, '李航');
  assert.equal(last.publisher, '清华大学出版社');
  assert.equal(last.year, '2019');
  assert.equal(sections[0].subjectRaw, '829 数据工程基础');
  assert.equal(sections[0].stage, '初试');
});

test('rejects year-only and exam-scope fragments', () => {
  assert.equal(parseLooseCitation('南阳师范学院文学院2026年7月25日'), null);
  const citation = parseLooseCitation('2.普通化学（笔试，侧重运用化学知识分析和解决问题），参阅华彤文等，普通化学原理（第4版），北京大学出版社（2013）。');
  assert.equal(citation.title, '普通化学原理');
  assert.equal(citation.publisher, '北京大学出版社');
  assert.equal(citation.year, '2013');
});

test('accepts an institutional author after the title', () => {
  const [book] = parseCitations('《普通高中体育与健康课程标准（2017年版）》；中华人民共和国教育部，人民教育出版社（2018）');
  assert.equal(book.authors, '中华人民共和国教育部');
  assert.equal(book.publisher, '人民教育出版社');
  assert.equal(book.year, '2018');
});

test('keeps inline tags together when converting html', () => {
  const text = htmlToText('<p>作者：<span>张三</span>：《测试书》</p><p>出版社：测试出版社</p>');
  assert.match(text, /作者：张三：《测试书》/);
});

test('parses major labels with letter codes and nested colleges', () => {
  assert.deepEqual(parseMajorLabel('085411 大数据技术与工程 (卓越工程师学院)'), {
    code: '085411',
    name: '大数据技术与工程',
    college: '卓越工程师学院',
  });
});

import { createHost } from './lib/host.mjs';
test('same subject keeps different editions and authors as separate citations', () => {
  const host=createHost();
  const citation={title:'数学分析',publisher:'高等教育出版社',year:'2020',volume:'上册',authors:'甲',edition:'第2版'};
  const sections=[citation,{...citation,edition:'第3版'},{...citation,authors:'乙'},citation].map(citation=>({subjectRaw:'601 数学分析',stage:'初试',citation}));
  assert.equal(host.groupSubjects(sections)[0].books.length,3);
});
