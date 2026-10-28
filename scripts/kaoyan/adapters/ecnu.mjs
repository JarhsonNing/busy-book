// 定制采集插件：华东师范大学研究生院招生目录（结构化表）。
// 研招网接口是"目录树 + 逐专业页"，通用正文解析拿不到，故做成定制插件作为示例。
export default {
  meta: {
    key: 'ecnu',
    name: '华东师范大学 · 研究生院',
    description: '研招网结构化招生目录（目录树 + 逐专业页），定制适配器示例。',
    // limit / delay / year / refresh 由面板的全局选项提供（映射到 host），此处无需重复声明。
    fields: [],
  },

  async run({ host }) {
    const ECNU_BASE = 'https://yjszs-ks.ecnu.edu.cn';
    const year = host.year;
    const flattenLeaves = (nodes) => {
      const leaves = [];
      const walk = (list) => {
        for (const node of list) {
          if (Array.isArray(node.children) && node.children.length > 0) walk(node.children);
          else leaves.push(node);
        }
      };
      walk(nodes);
      return leaves;
    };
    const pageUrl = (leaf) => {
      const base = `${ECNU_BASE}/zsml/sszsml/zsmlYjfxYjxk/${year}/${leaf.pId}/${leaf.id}`;
      return leaf.scode ? `${base}?ksfs=${encodeURIComponent(leaf.scode)}` : base;
    };

    const treeUrl = `${ECNU_BASE}/zsml/getTreeDataYjxk`;
    const treeText = await host.fetchCached(treeUrl, {
      method: 'POST',
      form: { nd: year, zslb: '1' },
      cacheKey: `ecnu-tree-${year}`,
      type: 'json',
      referer: `${ECNU_BASE}/zsml/sszsml`,
    });
    const leaves = flattenLeaves(JSON.parse(treeText));
    const selected = host.limit > 0 ? leaves.slice(0, host.limit) : leaves;
    console.error(`ECNU ${year}: ${leaves.length} 个专业页，采集 ${selected.length}`);

    const programs = [];
    const errors = [];
    for (const [index, leaf] of selected.entries()) {
      try {
        const { code, name, college } = host.parseMajorLabel(leaf.name);
        const html = await host.fetchCached(pageUrl(leaf), {
          cacheKey: `ecnu-${year}-${leaf.id}`,
          type: 'html',
          referer: `${ECNU_BASE}/zsml/sszsml`,
        });
        const subjects = host.groupSubjects(host.extractBookSections(host.htmlToText(html)));
        programs.push({ college, majorCode: code, majorName: name, pageUrl: pageUrl(leaf), subjects });
        if ((index + 1) % 20 === 0 || index + 1 === selected.length) {
          console.error(`  [${index + 1}/${selected.length}] ${leaf.name}`);
        }
      } catch (error) {
        errors.push({ id: leaf.id, name: leaf.name, error: error.message });
      }
    }

    const stats = host.summarize(programs);
    return {
      slug: 'ecnu',
      payload: {
        school: '华东师范大学',
        year: Number(year),
        collectedAt: new Date().toISOString(),
        source: { type: 'official-site', adapter: 'ecnu', entryUrl: `${ECNU_BASE}/zsml/sszsml`, treeUrl },
        programs,
        stats,
        errors,
      },
    };
  },
};
