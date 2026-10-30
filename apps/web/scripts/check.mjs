// Static checks for apps/web, on the TypeScript compiler API (no ESLint, so
// no extra dependency). Exits 1 with one line per problem.
//
// - V1: the envelope types in src/envelope.ts have exactly the schema's
//   properties and enums (contracts/inventory.v1.schema.json).
// - V3: no raw HTML injection, no eval, no `javascript:` URLs.
// - V10: imports stay inside apps/web; only src/api.ts touches fetch; no
//   storage, worker, push or socket APIs; no absolute URLs; no sorting; a
//   session's status is read only to show it (statusMarker or <Marker>).
// - Types: no `any`; type assertions only where JSON is checked (src/envelope.ts).
// - Design rules: the spacing scale and line heights in multiples of 4.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import ts from 'typescript';

const WEB = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SCHEMA = resolve(WEB, '../../contracts/inventory.v1.schema.json');
const problems = [];
const problem = (file, node, text) => {
  const sf = node?.getSourceFile?.();
  const line = sf ? sf.getLineAndCharacterOfPosition(node.getStart()).line + 1 : 0;
  problems.push(`${relative(WEB, file)}${line ? `:${line}` : ''}: ${text}`);
};

const walk = (dir) =>
  readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });

// The checked sources: every file under pages/ and src/, which is what ships.
// Fail closed: a file the checks cannot read (not .ts or .tsx) is a problem.
const sources = ['pages', 'src'].flatMap((d) => walk(join(WEB, d)));
for (const f of sources) {
  if (!/\.tsx?$/.test(f)) problems.push(`${relative(WEB, f)}: a source file the checks cannot read (only .ts and .tsx are allowed)`);
}
const files = sources.filter((f) => /\.tsx?$/.test(f));

const program = ts.createProgram(files, {
  strict: true,
  jsx: ts.JsxEmit.ReactJSX,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  target: ts.ScriptTarget.ES2022,
  allowImportingTsExtensions: true,
  noEmit: true,
  skipLibCheck: true,
  types: [],
});
const checker = program.getTypeChecker();

const ALLOWED_PACKAGES = new Set(['react', 'react-dom', 'react-dom/client', 'next/app', 'next/document', 'next/head']);
const HTML_SINKS = new Set([
  'innerHTML',
  'outerHTML',
  'insertAdjacentHTML',
  'srcdoc',
  'createContextualFragment',
  'setHTMLUnsafe',
  'parseFromString',
  'dangerouslySetInnerHTML',
]);
const GLOBALS = new Set(['window', 'globalThis', 'self', 'document', 'top', 'parent', 'frames']);
const URL_LIKE = /^([a-z][a-z0-9+.-]*:)?\/\/|^(javascript|data|blob):/i;
const BANNED_NAMES = new Set([
  'eval',
  'localStorage',
  'sessionStorage',
  'indexedDB',
  'caches',
  'serviceWorker',
  'Notification',
  'PushManager',
  'sendBeacon',
  'WebSocket',
  'EventSource',
  'XMLHttpRequest',
  'importScripts',
  'Worker',
  'SharedWorker',
  'BroadcastChannel',
  'cookie',
  'DOMParser',
]);
const ORDERING = new Set(['sort', 'toSorted', 'reverse', 'toReversed', 'localeCompare']);

for (const file of files) {
  const sf = program.getSourceFile(file);
  if (!sf) {
    problems.push(`${relative(WEB, file)}: not in the checked program`);
    continue;
  }
  const rel = relative(WEB, file);
  for (const c of sf.text.matchAll(/@ts-ignore|@ts-expect-error|@ts-nocheck|eslint-disable/g)) {
    problems.push(`${rel}: a checker is silenced (${c[0]})`);
  }
  const visit = (node) => {
    // Imports: relative inside apps/web, or an allowed package.
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      const spec = node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier) ? node.moduleSpecifier.text : null;
      if (spec !== null) {
        if (spec.startsWith('.')) {
          const target = resolve(dirname(file), spec);
          if (!target.startsWith(WEB + '/')) problem(file, node, `an import outside apps/web: ${spec}`);
        } else if (!ALLOWED_PACKAGES.has(spec)) problem(file, node, `an import of ${spec}`);
      }
    }
    if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) problem(file, node, 'a dynamic import');
    // V3: raw HTML.
    if (ts.isJsxAttribute(node) && node.name.getText() === 'dangerouslySetInnerHTML') problem(file, node, 'dangerouslySetInnerHTML');
    if (ts.isJsxAttribute(node) && /^on[A-Z]/.test(node.name.getText()) && node.initializer && ts.isStringLiteral(node.initializer)) {
      problem(file, node, 'an event handler given as a string');
    }
    if ((ts.isPropertyAccessExpression(node) || ts.isPropertyAssignment(node)) && HTML_SINKS.has(node.name.getText().replace(/['"]/g, ''))) {
      problem(file, node, `an HTML sink: ${node.name.getText()}`);
    }
    // obj['name'] is obj.name: the same rules apply. Computed access on a global is refused outright.
    if (ts.isElementAccessExpression(node)) {
      const key = node.argumentExpression;
      const name = ts.isStringLiteralLike(key) ? key.text : null;
      if (GLOBALS.has(node.expression.getText())) problem(file, node, `computed access on ${node.expression.getText()}`);
      else if (name !== null && (HTML_SINKS.has(name) || BANNED_NAMES.has(name) || ORDERING.has(name) || name === 'fetch' || name === 'status')) {
        problem(file, node, `${name} through a computed key`);
      } else if (name === null && !ts.isNumericLiteral(key)) {
        // A computed key could name anything above. Allowed: numbers, and a
        // finite union of string literals, each of them checked. Arbitrary
        // string keys only in src/envelope.ts, the JSON validator, which reads.
        const type = checker.getTypeAtLocation(key);
        const members = type.isUnion() ? type.types : [type];
        const literals = members.every((t) => t.isStringLiteral()) ? members.map((t) => t.value) : null;
        const numeric = (type.flags & ts.TypeFlags.NumberLike) !== 0;
        if (literals) {
          for (const lit of literals) {
            if (HTML_SINKS.has(lit) || BANNED_NAMES.has(lit) || ORDERING.has(lit) || lit === 'fetch' || lit === 'status') problem(file, node, `${lit} through a computed key`);
          }
        } else if (!numeric && rel !== 'src/envelope.ts') problem(file, node, 'a computed member access with a string key');
      }
    }
    // const { status } = session, const { fetch } = window, and the like.
    if (ts.isBindingElement(node) && ts.isObjectBindingPattern(node.parent)) {
      const key = (node.propertyName ?? node.name).getText().replace(/['"]/g, '');
      const from = checker.getTypeAtLocation(node.parent);
      const fromSession = (from.aliasSymbol?.name ?? from.getSymbol()?.name) === 'Session';
      if ((key === 'status' && fromSession) || key === 'fetch' || HTML_SINKS.has(key) || BANNED_NAMES.has(key)) problem(file, node, `${key} destructured`);
    }
    if (ts.isPropertyAccessExpression(node) && ['write', 'writeln'].includes(node.name.text) && node.expression.getText() === 'document') {
      problem(file, node, 'document.write');
    }
    if (ts.isNewExpression(node) && node.expression.getText() === 'Function') problem(file, node, 'new Function');
    // Function(...) without new, globalThis.Function, and the constructor route to it.
    if (ts.isIdentifier(node) && node.text === 'Function' && !ts.isTypeReferenceNode(node.parent) && !ts.isNewExpression(node.parent)) {
      problem(file, node, 'Function');
    }
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression) && node.expression.name.text === 'constructor') {
      problem(file, node, 'a call through .constructor');
    }
    if (ts.isCallExpression(node) && ['setTimeout', 'setInterval'].includes(node.expression.getText().replace(/^(window|globalThis)\./, ''))) {
      const first = node.arguments[0];
      if (first && (ts.isStringLiteral(first) || ts.isTemplateExpression(first) || ts.isNoSubstitutionTemplateLiteral(first))) {
        problem(file, node, 'a timer given a string');
      }
    }
    // V10: requests, storage, workers, sockets.
    if (ts.isIdentifier(node) && node.text === 'fetch' && rel !== 'src/api.ts') {
      const p = node.parent;
      const member =
        (ts.isPropertyAccessExpression(p) && p.name === node && !['window', 'globalThis', 'self'].includes(p.expression.getText())) ||
        ((ts.isPropertyAssignment(p) || ts.isPropertySignature(p) || ts.isPropertyDeclaration(p)) && p.name === node);
      if (!member) problem(file, node, 'fetch outside src/api.ts');
    }
    if (ts.isIdentifier(node) && BANNED_NAMES.has(node.text) && !ts.isPropertySignature(node.parent)) {
      problem(file, node, `a banned browser API: ${node.text}`);
    }
    if (ts.isPropertyAccessExpression(node) && ORDERING.has(node.name.text)) problem(file, node, `re-ordering: .${node.name.text}`);
    // No absolute or script URLs in the code.
    if ((ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) && URL_LIKE.test(node.text)) {
      problem(file, node, `an absolute or script URL: ${node.text}`);
    }
    if (ts.isTemplateExpression(node) && URL_LIKE.test(node.head.text)) problem(file, node, `an absolute or script URL: ${node.head.text}…`);
    // A session's status is read only to show it.
    if (ts.isPropertyAccessExpression(node) && node.name.text === 'status') {
      const owner = checker.getTypeAtLocation(node.expression);
      const name = owner.aliasSymbol?.name ?? owner.getSymbol()?.name;
      if (name === 'Session') {
        const call = node.parent;
        const viaCall = ts.isCallExpression(call) && call.expression.getText() === 'statusMarker' && call.arguments[0] === node;
        // <Marker status={session.status} />, which calls statusMarker.
        const attr = ts.isJsxExpression(call) ? call.parent : null;
        const viaMarker =
          attr !== null && ts.isJsxAttribute(attr) && attr.name.getText() === 'status' && attr.parent.parent.tagName?.getText() === 'Marker';
        if (!viaCall && !viaMarker) problem(file, node, 'a session status read other than statusMarker(session.status) or <Marker status={…}>');
      }
    }
    // Types.
    if (node.kind === ts.SyntaxKind.AnyKeyword) problem(file, node, 'the any type');
    if ((ts.isAsExpression(node) || ts.isTypeAssertionExpression(node)) && rel !== 'src/envelope.ts') {
      if (!(ts.isAsExpression(node) && node.type.getText() === 'const')) problem(file, node, 'a type assertion outside src/envelope.ts');
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
}

// V1: the envelope types against the schema.
const schema = JSON.parse(readFileSync(SCHEMA, 'utf8'));
const defs = schema.$defs;
const propsOf = (def, seen = new Set()) => {
  if (def.$ref) {
    const name = def.$ref.replace('#/$defs/', '');
    if (seen.has(name)) return new Set();
    seen.add(name);
    return propsOf(defs[name], seen);
  }
  const out = new Set(Object.keys(def.properties ?? {}));
  for (const branch of def.oneOf ?? []) for (const p of propsOf(branch, seen)) out.add(p);
  return out;
};
const envelopeFile = join(WEB, 'src/envelope.ts');
const esf = program.getSourceFile(envelopeFile);
const aliases = new Map();
ts.forEachChild(esf, (n) => {
  if (ts.isTypeAliasDeclaration(n)) aliases.set(n.name.text, n);
});
const tsProps = (typeNode, seen = new Set()) => {
  const out = new Set();
  if (!typeNode) return out;
  if (ts.isTypeLiteralNode(typeNode)) {
    for (const m of typeNode.members) if (m.name) out.add(m.name.getText().replace(/['"]/g, ''));
  } else if (ts.isUnionTypeNode(typeNode) || ts.isIntersectionTypeNode(typeNode)) {
    for (const t of typeNode.types) for (const p of tsProps(t, seen)) out.add(p);
  } else if (ts.isTypeReferenceNode(typeNode)) {
    const name = typeNode.typeName.getText();
    if (aliases.has(name) && !seen.has(name)) {
      seen.add(name);
      for (const p of tsProps(aliases.get(name).type, seen)) out.add(p);
    }
  } else if (ts.isParenthesizedTypeNode(typeNode)) {
    for (const p of tsProps(typeNode.type, seen)) out.add(p);
  }
  return out;
};
const pascal = (s) => s.replace(/(^|_)([a-z])/g, (_, __, c) => c.toUpperCase());
for (const [name, def] of Object.entries(defs)) {
  const alias = aliases.get(pascal(name));
  if (!alias) {
    problems.push(`src/envelope.ts: no type ${pascal(name)} for the schema's ${name}`);
    continue;
  }
  const want = [...propsOf(def)].sort().join(', ');
  const have = [...tsProps(alias.type)].sort().join(', ');
  if (want !== have) problems.push(`src/envelope.ts: ${pascal(name)} has {${have}}, the schema's ${name} has {${want}}`);
}
const gapKinds = esf.text.match(/GAP_KINDS = \[([\s\S]*?)\]/)?.[1].match(/'([a-z_]+)'/g)?.map((s) => s.slice(1, -1)) ?? [];
if (gapKinds.join() !== defs.gap.properties.kind.enum.join()) problems.push('src/envelope.ts: GAP_KINDS differs from the schema\'s gap kinds');
const coverageStates = aliases.get('Coverage')?.getText().match(/state: ([^;]+);/)?.[1].match(/'([a-z]+)'/g)?.map((s) => s.slice(1, -1)) ?? [];
if (coverageStates.join() !== defs.coverage.properties.state.enum.join()) problems.push('src/envelope.ts: Coverage.state differs from the schema');
if (!esf.text.includes(`SCHEMA_ID = '${defs.envelope.properties.schema.const}'`)) problems.push('src/envelope.ts: SCHEMA_ID differs from the schema');

// Design rules: spacing scale and line heights.
const SCALE = new Set([0, 4, 8, 12, 16, 24, 32, 48]);
const SPACING = /^(margin|padding|gap|row-gap|column-gap)(-(top|right|bottom|left|block|inline)(-(start|end))?)?$/;
for (const css of walk(join(WEB, 'styles')).filter((f) => f.endsWith('.css'))) {
  const text = readFileSync(css, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
  for (const block of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = block[1].trim();
    for (const decl of block[2].split(';')) {
      const m = /^\s*([a-z-]+)\s*:\s*(.+?)\s*$/.exec(decl);
      if (!m) continue;
      const [, prop, value] = m;
      const px = [...value.matchAll(/(-?\d*\.?\d+)px/g)].map((x) => Number(x[1]));
      if (SPACING.test(prop)) {
        const chip = selector === '.mk' && prop === 'padding' && value === '4px 6px';
        if (!chip && px.some((v) => !SCALE.has(v))) problems.push(`${relative(WEB, css)}: ${selector} { ${prop}: ${value} } is off the 4-point scale`);
      }
      if (prop === 'line-height' && px.some((v) => v % 4 !== 0)) problems.push(`${relative(WEB, css)}: ${selector} line-height ${value} is not a multiple of 4`);
      if (prop === 'font') {
        const lh = /\/(\d+)px/.exec(value);
        if (lh && Number(lh[1]) % 4 !== 0) problems.push(`${relative(WEB, css)}: ${selector} font line height ${lh[1]}px is not a multiple of 4`);
      }
    }
  }
}

if (problems.length) {
  for (const p of problems) console.log(p);
  console.log(`check: ${problems.length} problem(s)`);
  process.exit(1);
}
console.log(`check: ${files.length} source files and the stylesheet pass (V1 schema parity, V3, V10, types, spacing)`);
