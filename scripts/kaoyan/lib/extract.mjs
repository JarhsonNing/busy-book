const ENTITY_MAP = {
  nbsp: ' ', amp: '&', lt: '<', gt: '>', quot: '"', apos: "'",
  mdash: '—', ndash: '–', ldquo: '“', rdquo: '”', lsquo: '‘', rsquo: '’',
  hellip: '…', middot: '·', times: '×', deg: '°',
};

export function decodeEntities(text) {
  return text.replace(/&(#x?[0-9a-f]+|[a-z]+);/gi, (match, code) => {
    const key = String(code).toLowerCase();
    if (key.startsWith('#x')) return String.fromCodePoint(parseInt(key.slice(2), 16));
    if (key.startsWith('#')) return String.fromCodePoint(parseInt(key.slice(1), 10));
    return ENTITY_MAP[key] ?? match;
  });
}

export function htmlToText(html) {
  return decodeEntities(
    html
      .replace(/<script[\s\S]*?<\/script>/gi, '')
      .replace(/<style[\s\S]*?<\/style>/gi, '')
      .replace(/<br\s*\/?>/gi, '\n')
      .replace(/<\/(p|div|tr|li|h[1-6]|td|th|section|article)>/gi, '\n')
      .replace(/<[^>]+>/g, ''),
  )
    .replace(/\u00a0/g, ' ')
    .replace(/[ \t]+/g, ' ')
    .replace(/\n{2,}/g, '\n')
    .trim();
}

const EDITION_RE = /(第[一二三四五六七八九十百0-9]+版|增订[一二三四五六七八九十0-9]*版|修订版)/;
const YEAR_RE = /((?:19|20)\d{2})\s*年?/;
const ROLE_RE = /(主编|编著|著|编)$/;
const PUBLISHER_RE = /^([\u4e00-\u9fa5A-Za-z0-9&·]+?(?:大学出版社|出版社|书局|出版公司|印书馆|出版))/;
const BOOK_HEADING_RE = /(?:参考|推荐|指定|主要)[^，,。；;：:\n]{0,8}(?:书目|书单|教材|用书|书)/;
const SECTION_STOP_RE = /^(复试范围|初试考试范围|考试范围|考试科目|考试内容|招生人数|备注|联系方式|上一篇|下一篇)/;
const LOOSE_REJECT_RE = /不指定|无参考|见.{0,10}(?:简章|目录|大纲)|另行|考试大纲|考试范围|考试内容|招生人数|复试范围|初试科目|参考书仅供|拓展阅读/;
const SUBJECT_LINE_RE = /^\d{3,4}(?:[-－]\s*|\s+)\S+/;
const GBT_RE = /^(.{1,120}?)[.．]\s*(.+?)\s*\[(?:M|G|Z|J|D|C|N|P|R|S|A|EB\/OL|DB\/OL|J\/OL|M\/OL|CP\/D)\]/i;

function cleanRaw(raw) {
  return raw
    .replace(/^[\s\d.、．,，;；:：()（）[\]【】①-⑩]+/, '')
    .replace(/\s+/g, ' ')
    .trim();
}

function findPublisher(text) {
  const segments = text.split(/[，,；;。：:（）()\[\]【】.．\s]+/).map((item) => item.trim()).filter(Boolean);
  for (const segment of segments) {
    const match = segment.match(PUBLISHER_RE);
    if (match) return match[1];
  }
  return null;
}

function findYear(text) {
  return text.match(YEAR_RE)?.[1] ?? null;
}

function cleanAuthor(value) {
  let author = value
    .replace(/^[\s\d.、．,，;；:：()（）[\]【】①-⑩]+/, '')
    .replace(/^(?:作者|编者|著者)[:：]\s*/, '')
    .replace(/[，,；;。：:\s]+$/, '')
    .trim();
  if (/^[\u4e00-\u9fa5]{1,3}\)/.test(author)) author = `(${author}`;
  if (!author || author.length > 50) return null;
  return author;
}

const AUTHOR_NOISE_RE = /参考|教材|科目|考试|书目|出版社|书局|模块|部分|内容|笔试|面试|口试|实验|参阅|考查|范围|编写组|委员会|课题组|项目组|标准/;

function isAuthorNoise(value) {
  return !value || AUTHOR_NOISE_RE.test(value) || value.length > 50;
}

const PERSON_LIST_RE = /^[\u4e00-\u9fa5·]{2,4}(?:[、，,][\u4e00-\u9fa5·]{2,4})*(?:等)?$/;

function parsePersonList(segment) {
  const trimmed = segment.trim();
  if (!PERSON_LIST_RE.test(trimmed) || isAuthorNoise(trimmed)) return null;
  return trimmed;
}

function splitTitle(titleRaw) {
  let title = titleRaw.trim();
  let volume = null;
  let edition = null;

  const editionParen = title.match(/[（(](第[一二三四五六七八九十百0-9]+版|增订[一二三四五六七八九十0-9]*版|修订版)[）)]/);
  if (editionParen) {
    edition = editionParen[1];
    title = title.replace(editionParen[0], '').trim();
  }

  const paren = title.match(/[（(]([^（）()]+)[）)]\s*$/);
  if (paren) {
    const inner = paren[1].trim();
    if (/(册|卷|部分|篇|上|下)$/.test(inner) && inner.length <= 24) {
      volume = inner;
      title = title.slice(0, paren.index).trim();
    }
  }
  return { title, volume, edition };
}

function parseAuthorBefore(raw) {
  const cleaned = cleanAuthor(raw);
  if (isAuthorNoise(cleaned)) return { authors: null, role: null };
  const role = cleaned.match(ROLE_RE)?.[1] ?? null;
  const authors = role ? cleaned.slice(0, -role.length).trim() : cleaned;
  return { authors: authors || null, role };
}

function parseAuthorAfter(raw) {
  const match = raw.match(/^\s*[，,]?\s*([^，,。；;：:（(]{1,30}?)(等)?\s*(主编|编著|著|编)/);
  if (!match) return { authors: null, role: null };
  const authors = (match[1].trim() + (match[2] ?? '')).trim();
  return { authors: authors || null, role: match[3] };
}

function findAuthorList(afterRaw) {
  const segments = afterRaw.split(/[，,]/).map((item) => item.trim()).filter(Boolean);
  for (const segment of segments) {
    if (/^[（(][^）)]*[）)]$/.test(segment)) continue;
    const cleaned = segment.replace(/^[；;，,\s]+|[；;，,\s]+$/g, '');
    if (!cleaned) continue;
    if (findPublisher(cleaned) || /出版社|书局|出版公司/.test(cleaned) || findYear(cleaned)) break;
    const names = parsePersonList(cleaned);
    if (names) return names;
    if (/^(?:中华人民共和国|中国|国家|教育部|国务院|全国)[\u4e00-\u9fa5]{2,30}(?:部|委|局|院|会|中心|大学|学院)$/.test(cleaned)) {
      return cleaned;
    }
  }
  return null;
}

function buildCitation({ raw, title, volume, authors, authorRole, publisher, edition, year, base }) {
  let confidence = base;
  if (authors) confidence += 0.2;
  if (publisher) confidence += 0.25;
  if (year) confidence += 0.1;
  if (edition || volume) confidence += 0.03;
  return {
    raw,
    title,
    volume,
    authors: authors || null,
    authorRole: authorRole || null,
    publisher: publisher || null,
    edition: edition || null,
    year: year || null,
    confidence: Math.round(Math.min(0.97, confidence) * 100) / 100,
  };
}

export function parseCitations(raw) {
  const cleaned = cleanRaw(raw);
  const matches = [...cleaned.matchAll(/《([^》]+)》/g)];
  if (matches.length === 0) return [];

  const allYears = [...cleaned.matchAll(new RegExp(YEAR_RE.source, 'g'))].map((item) => item[1]);
  const sharedYear = new Set(allYears).size === 1 ? allYears[0] : null;

  return matches.map((match, index) => {
    const prevEnd = index === 0 ? 0 : matches[index - 1].index + matches[index - 1][0].length;
    const nextStart = index === matches.length - 1 ? cleaned.length : matches[index + 1].index;
    const beforeRaw = cleaned.slice(prevEnd, match.index);
    const afterRaw = cleaned.slice(match.index + match[0].length, nextStart);
    const authorRaw = afterRaw.replace(
      /^\s*[，,]?\s*[（(](?:第[一二三四五六七八九十百0-9]+版|增订[一二三四五六七八九十0-9]*版|修订版)[）)]\s*/,
      '',
    );

    const { title, volume, edition: titleEdition } = splitTitle(match[1]);
    if (title.length < 2) return null;

    let author = parseAuthorBefore(beforeRaw);
    if (!author.authors) {
      const labeled = authorRaw.match(/^\s*[，,]?\s*(?:作者|编者|著者)[:：]\s*(.+)$/);
      if (labeled) {
        const authors = cleanAuthor(labeled[1].split(/[。；;]/)[0]);
        if (authors && !isAuthorNoise(authors)) author = { authors, role: null };
      }
    }
    if (!author.authors) author = parseAuthorAfter(authorRaw);
    if (!author.authors) {
      const names = findAuthorList(authorRaw);
      if (names) author = { authors: names, role: null };
    }
    if (!author.authors) {
      const tail = beforeRaw.match(/(?:参阅|参见|见)?\s*([\u4e00-\u9fa5·]{2,5}?)(?:等)?\s*(主编|编著|著|编)\s*[，,。；;：:\s]*$/);
      if (tail && !isAuthorNoise(tail[1])) author = { authors: tail[1].trim(), role: tail[2] };
    }

    const publisher = findPublisher(afterRaw);
    const year = findYear(afterRaw) ?? sharedYear;
    const edition = titleEdition ?? afterRaw.match(EDITION_RE)?.[1] ?? null;

    return buildCitation({
      raw: cleaned,
      title,
      volume,
      authors: author.authors,
      authorRole: author.role,
      publisher,
      edition,
      year,
      base: 0.4,
    });
  }).filter(Boolean);
}

function looksLikePerson(value) {
  const cleaned = value.replace(/^\[[^\]]+\]\s*/, '').trim();
  if (/^[\u4e00-\u9fa5·]{2,6}$/.test(cleaned)) return true;
  if (/[A-Za-z].*[（(].*[）)]/.test(cleaned)) return true;
  return /^[A-Za-z][A-Za-z.\s-]{2,60}$/.test(cleaned);
}

const TITLE_NOISE_RE = /笔试|面试|口试|机试|参阅|参见|考查|考试范围|考试内容|考试科目|复试范围|初试范围/;

function isTitleNoise(piece) {
  if (!/[\u4e00-\u9fa5A-Za-z]/.test(piece)) return true;
  if (TITLE_NOISE_RE.test(piece)) return true;
  if (/^[（(]?\s*(?:19|20)\d{2}\s*(?:年(?:\s*\d{1,2}\s*月)?)?\s*[）)]?[.。]?$/.test(piece)) return true;
  if (/(?:19|20)\d{2}\s*年\s*\d{1,2}\s*月(?:\s*\d{1,2}\s*日)?/.test(piece)) return true;
  if (/^[（(]?(?:第[一二三四五六七八九十百0-9]+版|增订[一二三四五六七八九十0-9]*版|修订版)[）)]?$/.test(piece)) return true;
  return false;
}

function isCitationMetadata(piece) {
  if (piece.includes('《')) return false;
  if (/^(?:译者|作者|编者|著者)[:：]/.test(piece)) return true;
  if (PUBLISHER_RE.test(piece) || /^(?:19|20)\d{2}\s*(?:年|[（(])/.test(piece)) return true;
  if (/^(?:中华人民共和国|中国|国家|教育部|国务院|全国)/.test(piece) && /出版社|书局|出版公司|印书馆/.test(piece)) return true;
  return false;
}

function splitLooseMulti(text) {
  const parts = [];
  let start = 0;
  const boundary = /[）)]\s*(?=[\u4e00-\u9fa5·]{2,4}[.．][\u4e00-\u9fa5])/g;
  for (const match of text.matchAll(boundary)) {
    const left = text.slice(start, match.index + 1);
    if (!findPublisher(left) || !findYear(left)) continue;
    parts.push(left);
    start = match.index + match[0].length;
  }
  parts.push(text.slice(start));
  return parts.map((part) => part.trim()).filter(Boolean);
}

export function parseLooseCitation(raw) {
  let cleaned = cleanRaw(raw).replace(/^\[\d+\]\s*/, '').trim();
  if (cleaned.includes('《')) return null;
  if (cleaned.length < 4 || cleaned.length > 600) return null;
  if (LOOSE_REJECT_RE.test(cleaned)) return null;

  let authors = null;
  let title = null;
  let publisher = null;
  let year = null;
  let edition = null;

  const gbt = cleaned.match(GBT_RE);
  if (gbt) {
    authors = cleanAuthor(gbt[1]);
    if (isAuthorNoise(authors)) authors = null;
    title = gbt[2].trim();
    const rest = cleaned.slice(gbt.index + gbt[0].length);
    publisher = findPublisher(rest);
    year = findYear(rest);
    edition = rest.match(EDITION_RE)?.[1] ?? rest.match(/(?:^|[.．]\s*)(\d+版)(?=[.．]|$)/)?.[1] ?? null;
  } else {
    const pieces = cleaned
      .split(/[，；;。]\s*/)
      .map((piece) => piece.replace(/[。.．]+$/, '').trim())
      .filter(Boolean);

    const remaining = [];
    for (const piece of pieces) {
      if (/^(?:19|20)\d{2}\s*(?:年(?:\s*\d{1,2}\s*月)?|版)?\s*(?:[（(].*[）)]\s*)?[.。]?$/.test(piece)) {
        year = year ?? findYear(piece);
        continue;
      }
      const foundPublisher = findPublisher(piece);
      if (foundPublisher) {
        publisher = publisher ?? foundPublisher;
        if (!year) year = findYear(piece);
        let rest = piece
          .replace(foundPublisher, '')
          .replace(/^[\s，,。.．:：]+|[\s，,。.．:：]+$/g, '')
          .trim();
        rest = rest.replace(/^(?:19|20)\d{2}\s*(?:[（(][^）)]*[）)])?\s*/, '').trim();
        if (rest && !/^[\u4e00-\u9fa5]{2,4}[:：]$/.test(rest) && !/^(?:出版|发行)$/.test(rest)) {
          remaining.push(rest);
        }
        continue;
      }
      if (/^(?:\d+版|第[一二三四五六七八九十百0-9]+版|增订[一二三四五六七八九十0-9]*版|修订版)$/.test(piece)) {
        edition = edition ?? piece;
        continue;
      }
      remaining.push(piece);
    }

    const expanded = [];
    for (const piece of remaining) {
      const dot = piece.match(/^(.+?)[.．]\s*(?=[\u4e00-\u9fa5])/);
      if (dot) {
        const left = piece.slice(0, dot[0].length).replace(/[.．]\s*$/, '').trim();
        const right = piece.slice(dot[0].length).trim();
        if (left && right) {
          expanded.push(left, right);
          continue;
        }
      }
      expanded.push(piece);
    }
    remaining.length = 0;
    remaining.push(...expanded);

    const translators = new Set();
    for (let k = 0; k < remaining.length; k += 1) {
      if (!/(?:等)?译$/.test(remaining[k])) continue;
      translators.add(k);
      for (let p = k - 1; p >= 0 && p >= k - 2; p -= 1) {
        if (/^[\u4e00-\u9fa5·]{2,5}$/.test(remaining[p]) && !ROLE_RE.test(remaining[p])) {
          translators.add(p);
        } else break;
      }
    }

    const authorSegments = [];
    const titleCandidates = [];
    for (let k = 0; k < remaining.length; k += 1) {
      if (translators.has(k)) continue;
      const piece = remaining[k];
      if (/^(?:译者|译)[:：]/.test(piece)) continue;
      if (/^(?:参阅|参见|见)/.test(piece)) continue;
      if (/^(?:第\d+次印刷|ISBN|定价|开本|页数)/i.test(piece)) continue;

      const merged = piece.match(/^(.+?[）)])\s*([^，。]{2,40}?)(?:等)?\s*(主编|编著|著|编)$/);
      if (merged) {
        if (isTitleNoise(merged[1])) continue;
        titleCandidates.push(merged[1].trim());
        authorSegments.push(merged[2].trim());
        continue;
      }
      const explicit = piece.match(/^(.{1,60}?)(等)?\s*(主编|编著|著|编)$/);
      if (explicit) {
        authorSegments.push((explicit[1].trim() + (explicit[2] ?? '')).trim());
        continue;
      }
      const byLabel = piece.match(/^(?:作者|编者|著者)[:：]\s*(.+)$/);
      if (byLabel) {
        authorSegments.push(byLabel[1].trim());
        continue;
      }
      if (isTitleNoise(piece)) continue;
      titleCandidates.push(piece);
    }

    if (titleCandidates.length === 0) return null;
    title = titleCandidates[titleCandidates.length - 1];
    const extras = titleCandidates.slice(0, -1).filter(looksLikePerson);
    const names = [...new Set([...extras, ...authorSegments].map((name) => name.trim()).filter((name) => name && !isAuthorNoise(name)))];
    authors = names.length > 0 ? names.join('、') : null;
    if (!publisher) publisher = findPublisher(cleaned);
    if (!year) year = findYear(cleaned);
  }

  if (!title) return null;
  title = title
    .replace(/[，,]\s*(?:19|20)\d{2}\s*(?:年)?\s*(?:[（(][^）)]*[）)])?\s*$/, '')
    .replace(/[.．。]+$/, '')
    .trim();
  if (!publisher && !year) return null;
  const split = splitTitle(title);
  title = split.title;
  edition = edition ?? split.edition;
  if (title.length < 2 || title.length > 120) return null;
  if (isTitleNoise(title)) return null;
  if (/出版社|书局|出版公司/.test(title)) return null;
  if (/笔试|面试|参阅|考查|考试范围|考试内容/.test(title)) return null;
  if (/^(?:导师|备注|排名|电话|联系|地址)/.test(title)) return null;

  return buildCitation({
    raw: cleaned,
    title,
    volume: split.volume,
    authors,
    authorRole: null,
    publisher,
    edition,
    year,
    base: 0.35,
  });
}

export function parseCitation(raw) {
  return parseCitations(raw)[0] ?? parseLooseCitation(raw);
}

export function normalizeSubject(raw) {
  const text = raw.trim().replace(/^《([^》]+)》$/, '$1').replace(/的$/, '').trim();
  const match = text.match(/^(\d{3,4})[-－]?\s*(.+)$/);
  const subjectName = (match?.[2] ?? text)
    .replace(/[（(]满分\d+分[）)]/g, '')
    .replace(/[（(]共\d+分[）)]/g, '')
    .trim();
  return { subjectCode: match?.[1] ?? null, subjectName };
}

function findSubjectContext(lines, index, headingIndex) {
  const prefixRaw = lines[index].slice(0, headingIndex);
  let prefix = prefixRaw
    .replace(/^[\s\d.、．,，;；:：()（）[\]【】①-⑩]+/, '')
    .replace(/^《([^》]+)》.*$/, '$1');
  prefix = prefix
    .split(/[：:，,]/)[0]
    .replace(/[（(](?:笔试|面试|口试|机试|复试|初试)[）)]/g, '')
    .replace(/[，,、；;\s]+$/g, '')
    .replace(/的$/, '')
    .trim();

  let stage = /复试/.test(lines[index]) ? '复试' : /初试/.test(lines[index]) ? '初试' : null;
  let subjectLine = null;
  let colonLabel = null;
  const start = Math.max(0, index - 120);

  for (let i = index - 1; i >= start; i -= 1) {
    const line = lines[i];
    if (SUBJECT_LINE_RE.test(line)) {
      subjectLine = line;
      if (!stage) stage = '初试';
      break;
    }
    if (!colonLabel && /^[\u4e00-\u9fa5A-Za-z（）()·]{2,20}[:：]$/.test(line)) {
      const label = line.replace(/[:：]$/, '').trim();
      if (!/参考|教材|书目|科目|范围|内容|考试|招生|备注/.test(label)) colonLabel = label;
    }
    if (!stage && /^复试(范围|科目|内容|参考|笔试|面试)?/.test(line)) stage = '复试';
    else if (!stage && /^初试(范围|科目|内容|参考|笔试)?/.test(line)) stage = '初试';
  }

  if (stage === '复试' && colonLabel) {
    return { subjectRaw: colonLabel, stage };
  }

  const prefixUsable = prefix
    && prefix.length <= 40
    && !/^[IVXLC]+$/i.test(prefix)
    && !/[《》]/.test(prefix)
    && !/考试|内容|模块|参考|书目|科目|满分|招生|代码|不设定|大纲|要求/.test(prefix);
  return { subjectRaw: prefixUsable ? prefix : subjectLine ?? null, stage: stage ?? '初试' };
}

export function extractBookSections(text) {
  const lines = text.split('\n').map((line) => line.trim()).filter(Boolean);
  const sections = [];

  for (let i = 0; i < lines.length; i += 1) {
    const headingMatch = lines[i].match(BOOK_HEADING_RE);
    if (!headingMatch) continue;

    const context = findSubjectContext(lines, i, headingMatch.index);
    const chunkLines = [];
    let remainder = lines[i].slice(headingMatch.index + headingMatch[0].length);
    remainder = remainder.replace(/^\s*(?:[（(][^）)]*[）)])?\s*[:：]\s*/, '').trim();
    if (remainder) chunkLines.push(remainder);

    for (let j = i + 1; j < lines.length; j += 1) {
      const line = lines[j];
      if (BOOK_HEADING_RE.test(line)) break;
      if (SECTION_STOP_RE.test(line)) break;
      if (/^(?:19|20)\d{2}\s*年\s*\d{1,2}\s*月\s*\d{1,2}\s*日$/.test(line)) break;
      if (/^\d{3}(?:[-－]\s*|\s+)\S+/.test(line)) break;
      if (/拟考查|考核内容|考试内容|考试范围/.test(line)) break;
      chunkLines.push(line);
    }

    const pieces = [];
    const mergedLines = [];
    for (const line of chunkLines) {
      const last = mergedLines[mergedLines.length - 1];
      if (last !== undefined && (PUBLISHER_RE.test(line) || /^(?:19|20)\d{2}\s*(?:年|[（(])/.test(line))) {
        mergedLines[mergedLines.length - 1] = `${last}${/[，,；;。.．）)]$/.test(last) ? '' : '，'}${line}`;
      } else {
        mergedLines.push(line);
      }
    }
    for (const line of mergedLines) {
      const splitter = line.includes('《')
        ? /[；;。]\s*|\s+(?=\[\d+\]|\d+、)/
        : /[；;]\s*|\s+(?=\[\d+\]|\d+、)/;
      pieces.push(...line.split(splitter).map((piece) => piece.trim()).filter(Boolean));
    }

    const mergedPieces = [];
    for (const piece of pieces) {
      const prev = mergedPieces[mergedPieces.length - 1];
      if (prev && prev.includes('《') && isCitationMetadata(piece)) {
        mergedPieces[mergedPieces.length - 1] = `${prev}；${piece}`;
      } else {
        mergedPieces.push(piece);
      }
    }

    for (const piece of mergedPieces) {
      const chunks = piece.includes('《') ? [piece] : splitLooseMulti(piece);
      for (const chunk of chunks) {
        const citations = chunk.includes('《')
          ? parseCitations(chunk)
          : [parseLooseCitation(chunk)].filter(Boolean);
        for (const citation of citations) {
          sections.push({ ...context, citation });
        }
      }
    }
  }

  return sections;
}

export function parseMajorLabel(label) {
  const paren = label.match(/\s+[（(](.+)[）)]\s*$/);
  const college = paren ? paren[1].trim() : null;
  const rest = (paren ? label.slice(0, paren.index) : label).trim();
  const match = rest.match(/^([0-9]{4}[0-9A-Za-z]{0,2})\s+(.+)$/);
  if (!match) return { code: null, name: rest, college };
  return { code: match[1], name: match[2].trim(), college };
}
