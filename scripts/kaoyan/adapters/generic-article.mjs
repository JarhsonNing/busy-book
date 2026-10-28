// 通用采集插件：抓取单篇公告/通知正文里的参考书目。
// 适合"书目写在一段正文里"的学校（如院系研究生招生通知）。
// source.adapter 固定为 'article'，与既有数据保持一致，保证重建后 src_ id 不变。
export default {
  meta: {
    key: 'generic-article',
    name: '通用 · 单页公告',
    description: '抓取单篇公告/通知正文里的参考书目（书目写在正文段落里的学校适用）。',
    fields: [
      { name: 'url', label: '公告页 URL', required: true, placeholder: 'https://…/info/…' },
      { name: 'school', label: '学校名称', required: true, placeholder: '如 南阳师范学院' },
      { name: 'slug', label: '标识（留空按 URL 生成）' },
      { name: 'title', label: '标题（留空取正文首行）' },
      { name: 'college', label: '学院' },
      { name: 'majorCode', label: '专业代码', placeholder: '如 050100' },
      { name: 'majorName', label: '专业名称', placeholder: '如 中国语言文学' },
    ],
  },

  async run({ host, args }) {
    if (!args.url) throw new Error('通用公告插件需要 url');
    const slug = args.slug || host.sha1(args.url).slice(0, 10);
    const school = args.school || slug;
    const html = await host.fetchCached(args.url, { cacheKey: `article-${slug}-${host.year}`, type: 'html' });
    const text = host.htmlToText(html);
    const title =
      args.title ||
      text
        .split('\n')
        .map((line) => line.trim())
        .find(Boolean)
        ?.slice(0, 120) ||
      null;
    const sections = host.extractBookSections(text);
    const programs = [
      {
        college: args.college ?? null,
        majorCode: args.majorCode ?? null,
        majorName: args.majorName ?? null,
        title,
        pageUrl: args.url,
        subjects: host.groupSubjects(sections),
      },
    ];
    const stats = host.summarize(programs);
    return {
      slug,
      payload: {
        school,
        year: Number(host.year),
        collectedAt: new Date().toISOString(),
        source: { type: 'official-article', adapter: 'article', entryUrl: args.url, title },
        programs,
        stats,
        errors: [],
      },
    };
  },
};
