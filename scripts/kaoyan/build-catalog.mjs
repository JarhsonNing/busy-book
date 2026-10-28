import './lib/lifecycle.mjs';
// Usage: node scripts/kaoyan/build-catalog.mjs [--input data/kaoyan/books] [--out data/kaoyan/catalog]
import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { normalizeSubject } from './lib/extract.mjs';

const args = process.argv.slice(2);
const readArg = (name, fallback) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
};

const inputDir = path.resolve(readArg('--input', 'data/kaoyan/books'));
const outDir = path.resolve(readArg('--out', 'data/kaoyan/catalog'));

const hash = (text, prefix) => prefix + crypto.createHash('sha1').update(text).digest('hex').slice(0, 12);
const text = (value) => String(value ?? '').replace(/\u3000/g, ' ').replace(/\s+/g, ' ').trim();

const NATIONALITY_RE = /[\[【(（]\s*(?:美|英|法|德|日|俄|苏|加|澳|荷|意|西|葡|瑞典|瑞士|以色列|印度|韩|新加坡|马来西亚|巴西|墨西哥|南非|波兰|捷克|奥地利|比利时|丹麦|挪威|芬兰|希腊|爱尔兰|新西兰)\s*[\]】)）]/g;
const CN_DIGITS = { 一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9 };

function cnToNumber(value) {
  if (/^\d+$/.test(value)) return Number(value);
  if (value === '十') return 10;
  if (value.startsWith('十')) return 10 + (CN_DIGITS[value[1]] ?? 0);
  if (value.endsWith('十')) return (CN_DIGITS[value[0]] ?? 0) * 10;
  if (value.includes('十')) {
    const [tens, ones] = value.split('十');
    return (CN_DIGITS[tens] ?? 0) * 10 + (CN_DIGITS[ones] ?? 0);
  }
  return CN_DIGITS[value] ?? null;
}

function normalizeTitle(value) {
  return text(value)
    .replace(/^[《》]+|[《》]+$/g, '')
    .replace(/[（(][^）)]*第[一二三四五六七八九十百0-9]+版[^）)]*[）)]/g, '')
    .replace(/[（(](?:增订[一二三四五六七八九十0-9]*版|修订版)[）)]/g, '')
    .replace(/[.．。;；,，]+$/g, '')
    .trim();
}

function normalizeAuthors(value) {
  let raw = text(value)
    .replace(/^(?:作者|编者|著者)[:：]\s*/, '')
    .replace(NATIONALITY_RE, '')
    .replace(/[（(][^）)]{0,20}[）)]/g, '')
    .replace(/(?:等)?\s*(?:主编|编著|著|编)$/, '')
    .trim();
  if (!raw) return null;
  const names = raw
    .split(/[、，,;；/]/)
    .map((name) => name.replace(/^[\s.．]+|[\s.．]+$/g, '').replace(/等$/, '').trim())
    .filter(Boolean);
  return names.length ? [...new Set(names)].join('、') : null;
}

const PUBLISHER_ALIASES = new Map([
  ['高教社', '高等教育出版社'],
  ['人教社', '人民教育出版社'],
  ['机械工业出版', '机械工业出版社'],
  ['电子工业出版', '电子工业出版社'],
  ['清华大学出版', '清华大学出版社'],
  ['北京大学出版', '北京大学出版社'],
  ['中国人民大学出版', '中国人民大学出版社'],
  ['华东师范大学出版', '华东师范大学出版社'],
  ['北京师范大学出版', '北京师范大学出版社'],
  ['科学出版', '科学出版社'],
  ['人民邮电出版', '人民邮电出版社'],
  ['中国传媒大学出版', '中国传媒大学出版社'],
  ['上海教育出版', '上海教育出版社'],
  ['中国广播电视出版', '中国广播电视出版社'],
]);

function normalizePublisher(value) {
  let publisher = text(value)
    .replace(/^[，,；;。：:]+|[，,；;。：:]+$/g, '')
    .replace(/[（(][^）)]*[）)]/g, '')
    .trim();
  if (!publisher) return null;
  if (PUBLISHER_ALIASES.has(publisher)) return PUBLISHER_ALIASES.get(publisher);
  if (publisher.endsWith('出版')) publisher += '社';
  return publisher;
}

function normalizeEdition(value) {
  let edition = text(value).replace(/[（(）)]/g, '').trim();
  if (!edition) return null;
  edition = edition.replace(/第([一二三四五六七八九十百0-9]+)版/, (_, number) => {
    const parsed = cnToNumber(number);
    return `第${parsed ?? number}版`;
  });
  return edition;
}

function normalizeYear(value) {
  return String(value ?? '').match(/((?:19|20)\d{2})/)?.[1] ?? null;
}

function cleanSubjectName(value) {
  const name = text(value)
    .replace(/^[\d.\-、\s]+/, '')
    .replace(/[，,；;。：:]+$/g, '')
    .trim();
  if (!name) return '未注明科目';
  if (/大学.*硕士研究生|专业硕士研究生|初试范围|复试范围|研究方向\s*\d|考试范围|考试内容/.test(name)) return '未注明科目';
  if (name.length > 40) return '未注明科目';
  return name;
}

function buildSearchQuery(title, authors, publisher) {
  const firstAuthor = authors?.split('、')[0]?.replace(/[（(].*?[）)]/g, '').trim();
  return [title, firstAuthor, publisher].filter(Boolean).join(' ');
}

async function readSources() {
  const files = (await fs.readdir(inputDir)).filter((file) => file.endsWith('.json')).sort();
  const sources = [];
  for (const file of files) {
    const raw = JSON.parse(await fs.readFile(path.join(inputDir, file), 'utf8'));
    if (!raw.school || !Array.isArray(raw.programs)) continue;
    sources.push({ file, raw });
  }
  return sources;
}

async function main() {
  const loaded = await readSources();
  if (loaded.length === 0) throw new Error(`No school JSON files found in ${inputDir}`);

  const schools = new Map();
  const programs = new Map();
  const subjects = new Map();
  const books = new Map();
  const requirements = new Map();
  const sources = [];

  const addSchool = (name) => {
    const key = text(name);
    const id = hash(key, 'sch_');
    if (!schools.has(id)) schools.set(id, { id, name: key, programIds: new Set(), bookIds: new Set(), requirementIds: new Set() });
    return schools.get(id);
  };

  for (const { file, raw } of loaded) {
    const school = addSchool(raw.school);
    const year = Number(raw.year);
    const sourceId = hash(`${raw.school}|${year}|${raw.source?.adapter}|${raw.source?.entryUrl}`, 'src_');
    sources.push({
      id: sourceId,
      schoolId: school.id,
      year,
      type: raw.source?.type ?? null,
      adapter: raw.source?.adapter ?? null,
      entryUrl: raw.source?.entryUrl ?? null,
      collectedAt: raw.collectedAt ?? null,
      file,
    });

    for (const program of raw.programs) {
      const programKey = [school.id, year, program.college ?? '', program.majorCode ?? '', program.majorName ?? ''].join('|');
      const programId = hash(programKey, 'pg_');
      if (!programs.has(programId)) {
        programs.set(programId, {
          id: programId,
          schoolId: school.id,
          sourceId,
          year,
          college: program.college ?? null,
          majorCode: program.majorCode ?? null,
          majorName: program.majorName ?? null,
          title: program.title ?? null,
          pageUrl: program.pageUrl ?? null,
          subjectIds: new Set(),
        });
      }
      school.programIds.add(programId);

      for (const subject of program.subjects ?? []) {
        const stage = subject.stage === '复试' ? '复试' : '初试';
        const subjectCode = subject.subjectCode ?? normalizeSubject(subject.subjectName ?? '').subjectCode;
        const subjectName = cleanSubjectName(subject.subjectName);
        const subjectId = hash(`${programId}|${stage}|${subjectCode ?? ''}|${subjectName}`, 'sub_');
        if (!subjects.has(subjectId)) {
          subjects.set(subjectId, {
            id: subjectId,
            programId,
            stage,
            code: subjectCode ?? null,
            name: subjectName,
            raw: subject.subjectName ?? null,
            requirementIds: new Set(),
          });
          programs.get(programId).subjectIds.add(subjectId);
        }

        for (const book of subject.books ?? []) {
          const title = normalizeTitle(book.title);
          if (!title || title.length < 2) continue;
          const authors = normalizeAuthors(book.authors);
          const publisher = normalizePublisher(book.publisher);
          const edition = normalizeEdition(book.edition);
          const yearValue = normalizeYear(book.year);
          const volume = text(book.volume) || null;
          const bookKey = `${title}|${authors ?? ''}|${publisher ?? ''}|${edition ?? ''}|${yearValue ?? ''}|${volume ?? ''}`;
          const bookId = hash(bookKey, 'bk_');

          if (!books.has(bookId)) {
            books.set(bookId, {
              id: bookId,
              title,
              authors,
              publisher,
              edition,
              year: yearValue,
              volume,
              searchQuery: buildSearchQuery(title, authors, publisher),
              confidence: 0,
              editions: new Set(),
              years: new Set(),
              volumes: new Set(),
              schoolIds: new Set(),
              programIds: new Set(),
              subjectIds: new Set(),
              requirementIds: new Set(),
              rawVariants: new Set(),
            });
          }
          const entity = books.get(bookId);
          entity.authors ??= authors;
          entity.publisher ??= publisher;
          entity.edition ??= edition;
          entity.year ??= yearValue;
          entity.volume ??= volume;
          entity.confidence = Math.max(entity.confidence, Number(book.confidence) || 0);
          if (edition) entity.editions.add(edition);
          if (yearValue) entity.years.add(yearValue);
          if (volume) entity.volumes.add(volume);
          entity.schoolIds.add(school.id);
          entity.programIds.add(programId);
          entity.subjectIds.add(subjectId);
          if (book.raw) entity.rawVariants.add(text(book.raw));

          const requirementId = hash(`${subjectId}|${bookId}`, 'rq_');
          if (!requirements.has(requirementId)) {
            requirements.set(requirementId, {
              id: requirementId,
              subjectId,
              textbookId: bookId,
              raw: book.raw ?? null,
              confidence: Number(book.confidence) || 0,
              sourceUrl: program.pageUrl ?? raw.source?.entryUrl ?? null,
              sourceFile: file,
            });
            subjects.get(subjectId).requirementIds.add(requirementId);
            entity.requirementIds.add(requirementId);
            school.requirementIds.add(requirementId);
            school.bookIds.add(bookId);
          }
        }
      }
    }
  }

  for (const book of books.values()) {
    book.schoolCount = book.schoolIds.size;
    book.programCount = book.programIds.size;
    book.subjectCount = book.subjectIds.size;
    book.requirementCount = book.requirementIds.size;
    book.schools = [...book.schoolIds].map((id) => schools.get(id)?.name).filter(Boolean);
    book.editions = [...book.editions].sort();
    book.years = [...book.years].sort();
    book.volumes = [...book.volumes].sort();
    book.rawVariants = [...book.rawVariants].slice(0, 5);
    delete book.schoolIds;
    delete book.programIds;
    delete book.subjectIds;
    delete book.requirementIds;
  }

  for (const school of schools.values()) {
    school.programCount = school.programIds.size;
    school.bookCount = school.bookIds.size;
    school.requirementCount = school.requirementIds.size;
    delete school.programIds;
    delete school.bookIds;
    delete school.requirementIds;
  }

  const bookList = [...books.values()].sort((a, b) => b.requirementCount - a.requirementCount || a.title.localeCompare(b.title, 'zh'));
  const requirementList = [...requirements.values()];
  const subjectList = [...subjects.values()].map(({ requirementIds, ...subject }) => ({ ...subject, bookCount: requirementIds.size }));
  const programList = [...programs.values()].map(({ subjectIds, ...program }) => {
    const ownSubjects = [...subjectIds].map((id) => subjects.get(id));
    return {
      ...program,
      subjectCount: ownSubjects.length,
      bookCount: ownSubjects.reduce((sum, subject) => sum + subject.requirementIds.size, 0),
    };
  });

  const publisherStats = new Map();
  for (const book of bookList) {
    const key = book.publisher ?? '未注明出版社';
    const entry = publisherStats.get(key) ?? { publisher: key, books: 0, requirements: 0 };
    entry.books += 1;
    entry.requirements += book.requirementCount;
    publisherStats.set(key, entry);
  }

  const stats = {
    schools: schools.size,
    programs: programList.length,
    programsWithBooks: programList.filter((program) => program.bookCount > 0).length,
    subjects: subjectList.length,
    requirements: requirementList.length,
    books: bookList.length,
    booksWithAuthors: bookList.filter((book) => book.authors).length,
    booksWithPublisher: bookList.filter((book) => book.publisher).length,
    booksWithYear: bookList.filter((book) => book.year).length,
    bySchool: [...schools.values()].map((school) => ({
      school: school.name,
      programs: school.programCount,
      books: school.bookCount,
      requirements: school.requirementCount,
    })),
    topPublishers: [...publisherStats.values()].sort((a, b) => b.requirements - a.requirements).slice(0, 10),
    topBooks: bookList.slice(0, 10).map((book) => ({
      title: book.title,
      authors: book.authors,
      publisher: book.publisher,
      schools: book.schoolCount,
      requirements: book.requirementCount,
    })),
  };

  const payload = {
    generatedAt: new Date().toISOString(),
    sources,
    schools: [...schools.values()],
    programs: programList,
    subjects: subjectList,
    books: bookList,
    requirements: requirementList,
    stats,
  };

  await fs.mkdir(outDir, { recursive: true });
  await fs.writeFile(path.join(outDir, 'catalog.json'), JSON.stringify(payload, null, 2));

  const db = new DatabaseSync(path.join(outDir, 'catalog.db'));
  db.exec(`
    PRAGMA foreign_keys = ON;
    DROP VIEW IF EXISTS v_textbook_usage;
    DROP TABLE IF EXISTS requirements;
    DROP TABLE IF EXISTS subjects;
    DROP TABLE IF EXISTS programs;
    DROP TABLE IF EXISTS textbooks;
    DROP TABLE IF EXISTS sources;
    DROP TABLE IF EXISTS schools;

    CREATE TABLE schools (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL UNIQUE,
      program_count INTEGER NOT NULL DEFAULT 0,
      book_count INTEGER NOT NULL DEFAULT 0,
      requirement_count INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE sources (
      id TEXT PRIMARY KEY,
      school_id TEXT NOT NULL REFERENCES schools(id),
      year INTEGER NOT NULL,
      type TEXT,
      adapter TEXT,
      entry_url TEXT,
      collected_at TEXT,
      file TEXT
    );
    CREATE TABLE programs (
      id TEXT PRIMARY KEY,
      school_id TEXT NOT NULL REFERENCES schools(id),
      source_id TEXT REFERENCES sources(id),
      year INTEGER NOT NULL,
      college TEXT,
      major_code TEXT,
      major_name TEXT,
      title TEXT,
      page_url TEXT,
      subject_count INTEGER NOT NULL DEFAULT 0,
      book_count INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE subjects (
      id TEXT PRIMARY KEY,
      program_id TEXT NOT NULL REFERENCES programs(id),
      stage TEXT NOT NULL,
      code TEXT,
      name TEXT NOT NULL,
      raw TEXT,
      book_count INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE textbooks (
      id TEXT PRIMARY KEY,
      title TEXT NOT NULL,
      authors TEXT,
      publisher TEXT,
      edition TEXT,
      year TEXT,
      volume TEXT,
      editions TEXT,
      years TEXT,
      search_query TEXT,
      confidence REAL NOT NULL DEFAULT 0,
      school_count INTEGER NOT NULL DEFAULT 0,
      program_count INTEGER NOT NULL DEFAULT 0,
      subject_count INTEGER NOT NULL DEFAULT 0,
      requirement_count INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE requirements (
      id TEXT PRIMARY KEY,
      subject_id TEXT NOT NULL REFERENCES subjects(id),
      textbook_id TEXT NOT NULL REFERENCES textbooks(id),
      raw TEXT,
      confidence REAL NOT NULL DEFAULT 0,
      source_url TEXT,
      source_file TEXT,
      UNIQUE(subject_id, textbook_id)
    );

    CREATE INDEX idx_programs_school ON programs(school_id);
    CREATE INDEX idx_subjects_program ON subjects(program_id);
    CREATE INDEX idx_requirements_subject ON requirements(subject_id);
    CREATE INDEX idx_requirements_textbook ON requirements(textbook_id);
    CREATE INDEX idx_textbooks_title ON textbooks(title);

    CREATE VIEW v_textbook_usage AS
    SELECT
      t.id AS textbook_id,
      t.title,
      t.authors,
      t.publisher,
      sc.name AS school,
      p.year,
      p.college,
      p.major_code,
      p.major_name,
      sub.stage,
      sub.code AS subject_code,
      sub.name AS subject_name,
      r.confidence,
      r.source_url
    FROM requirements r
    JOIN textbooks t ON t.id = r.textbook_id
    JOIN subjects sub ON sub.id = r.subject_id
    JOIN programs p ON p.id = sub.program_id
    JOIN schools sc ON sc.id = p.school_id;
  `);

  const insertSchool = db.prepare('INSERT INTO schools(id, name, program_count, book_count, requirement_count) VALUES(?,?,?,?,?)');
  const insertSource = db.prepare('INSERT INTO sources(id, school_id, year, type, adapter, entry_url, collected_at, file) VALUES(?,?,?,?,?,?,?,?)');
  const insertProgram = db.prepare('INSERT INTO programs(id, school_id, source_id, year, college, major_code, major_name, title, page_url, subject_count, book_count) VALUES(?,?,?,?,?,?,?,?,?,?,?)');
  const insertSubject = db.prepare('INSERT INTO subjects(id, program_id, stage, code, name, raw, book_count) VALUES(?,?,?,?,?,?,?)');
  const insertBook = db.prepare('INSERT INTO textbooks(id, title, authors, publisher, edition, year, volume, editions, years, search_query, confidence, school_count, program_count, subject_count, requirement_count) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)');
  const insertRequirement = db.prepare('INSERT INTO requirements(id, subject_id, textbook_id, raw, confidence, source_url, source_file) VALUES(?,?,?,?,?,?,?)');

  db.exec('BEGIN');
  try {
    for (const school of schools.values()) insertSchool.run(school.id, school.name, school.programCount, school.bookCount, school.requirementCount);
    for (const source of sources) insertSource.run(source.id, source.schoolId, source.year, source.type, source.adapter, source.entryUrl, source.collectedAt, source.file);
    for (const program of programList) insertProgram.run(program.id, program.schoolId, program.sourceId, program.year, program.college, program.majorCode, program.majorName, program.title, program.pageUrl, program.subjectCount, program.bookCount);
    for (const subject of subjectList) insertSubject.run(subject.id, subject.programId, subject.stage, subject.code, subject.name, subject.raw, subject.bookCount);
    for (const book of bookList) insertBook.run(book.id, book.title, book.authors, book.publisher, book.edition, book.year, book.volume, JSON.stringify(book.editions), JSON.stringify(book.years), book.searchQuery, book.confidence, book.schoolCount, book.programCount, book.subjectCount, book.requirementCount);
    for (const requirement of requirementList) insertRequirement.run(requirement.id, requirement.subjectId, requirement.textbookId, requirement.raw, requirement.confidence, requirement.sourceUrl, requirement.sourceFile);
    db.exec('COMMIT');
  } catch (error) {
    db.exec('ROLLBACK');
    throw error;
  } finally {
    db.close();
  }

  console.log(JSON.stringify({
    catalog: path.relative(process.cwd(), path.join(outDir, 'catalog.json')),
    database: path.relative(process.cwd(), path.join(outDir, 'catalog.db')),
    ...stats,
    bySchool: undefined,
    topPublishers: undefined,
    topBooks: undefined,
  }, null, 2));
  console.error(`Top books: ${stats.topBooks.map((book) => `${book.title}(${book.requirements})`).join(', ')}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
