// Usage:
//   node scripts/kaoyan/collect-books.mjs ecnu --year 2027 [--limit 0] [--delay 800] [--refresh]
//   node scripts/kaoyan/collect-books.mjs article --url <url> --school <name> --slug <slug> [--title <t>]
//       [--major-code 050100] [--major-name 中国语言文学] [--college 文学院] [--year 2027]
// Shared fetch/cache/extract machinery lives in lib/host.mjs (single source of truth).
import path from 'node:path';
import { createHost } from './lib/host.mjs';

const ECNU_BASE = 'https://yjszs-ks.ecnu.edu.cn';

const args = process.argv.slice(2);
const mode = args[0] && !args[0].startsWith('--') ? args[0] : 'ecnu';
const readArg = (name, fallback = null) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
};
const hasFlag = (name) => args.includes(name);

const host = createHost({
  year: readArg('--year', '2027'),
  delay: readArg('--delay', '800'),
  limit: readArg('--limit', '0'),
  refresh: hasFlag('--refresh'),
});
const { year, limit } = host;

function flattenLeaves(nodes) {
  const leaves = [];
  const walk = (list) => {
    for (const node of list) {
      if (Array.isArray(node.children) && node.children.length > 0) walk(node.children);
      else leaves.push(node);
    }
  };
  walk(nodes);
  return leaves;
}

function ecnuPageUrl(leaf) {
  const base = `${ECNU_BASE}/zsml/sszsml/zsmlYjfxYjxk/${year}/${leaf.pId}/${leaf.id}`;
  return leaf.scode ? `${base}?ksfs=${encodeURIComponent(leaf.scode)}` : base;
}

async function collectEcnu() {
  const treeUrl = `${ECNU_BASE}/zsml/getTreeDataYjxk`;
  const treeText = await host.fetchCached(treeUrl, {
    method: 'POST',
    form: { nd: year, zslb: '1' },
    cacheKey: `ecnu-tree-${year}`,
    type: 'json',
    referer: `${ECNU_BASE}/zsml/sszsml`,
  });
  const tree = JSON.parse(treeText);
  const leaves = flattenLeaves(tree);
  const selected = limit > 0 ? leaves.slice(0, limit) : leaves;
  console.error(`ECNU ${year}: ${leaves.length} major pages, collecting ${selected.length}`);

  const programs = [];
  const errors = [];
  for (const [index, leaf] of selected.entries()) {
    try {
      const { code, name, college } = host.parseMajorLabel(leaf.name);
      const pageUrl = ecnuPageUrl(leaf);
      const html = await host.fetchCached(pageUrl, {
        cacheKey: `ecnu-${year}-${leaf.id}`,
        type: 'html',
        referer: `${ECNU_BASE}/zsml/sszsml`,
      });
      const sections = host.extractBookSections(host.htmlToText(html));
      const subjects = host.groupSubjects(sections);
      const bookCount = subjects.reduce((sum, subject) => sum + subject.books.length, 0);
      programs.push({ college, majorCode: code, majorName: name, pageUrl, subjects });
      console.error(`[${index + 1}/${selected.length}] ${leaf.name}: ${subjects.length} subjects, ${bookCount} books`);
    } catch (error) {
      errors.push({ id: leaf.id, name: leaf.name, error: error.message });
      console.error(`[${index + 1}/${selected.length}] ${leaf.name}: failed ${error.message}`);
    }
  }

  const stats = host.summarize(programs);
  const payload = {
    school: '华东师范大学',
    year: Number(year),
    collectedAt: new Date().toISOString(),
    source: { type: 'official-site', adapter: 'ecnu', entryUrl: `${ECNU_BASE}/zsml/sszsml`, treeUrl },
    programs,
    stats,
    errors,
  };
  const file = await host.writeResult('ecnu', payload);
  console.log(JSON.stringify({ file: path.relative(process.cwd(), file), ...stats, errors: errors.length }, null, 2));
}

async function collectArticle() {
  const url = readArg('--url');
  if (!url) throw new Error('article mode requires --url');
  const slug = readArg('--slug') || host.sha1(url).slice(0, 10);
  const school = readArg('--school') || slug;
  const html = await host.fetchCached(url, { cacheKey: `article-${slug}-${year}`, type: 'html' });
  const text = host.htmlToText(html);
  const title =
    readArg('--title') ||
    text
      .split('\n')
      .map((line) => line.trim())
      .find(Boolean)
      ?.slice(0, 120) ||
    null;
  const sections = host.extractBookSections(text);
  const programs = [
    {
      college: readArg('--college'),
      majorCode: readArg('--major-code'),
      majorName: readArg('--major-name'),
      title,
      pageUrl: url,
      subjects: host.groupSubjects(sections),
    },
  ];
  const stats = host.summarize(programs);
  const payload = {
    school,
    year: Number(year),
    collectedAt: new Date().toISOString(),
    source: { type: 'official-article', adapter: 'article', entryUrl: url, title },
    programs,
    stats,
    errors: [],
  };
  const file = await host.writeResult(slug, payload);
  console.log(JSON.stringify({ file: path.relative(process.cwd(), file), ...stats }, null, 2));
}

async function main() {
  if (mode === 'ecnu') await collectEcnu();
  else if (mode === 'article') await collectArticle();
  else throw new Error(`Unknown mode: ${mode}. Use "ecnu" or "article".`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
