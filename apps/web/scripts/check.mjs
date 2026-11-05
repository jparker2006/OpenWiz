

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import ts from 'typescript';

const WEB = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SCHEMA = resolve(WEB, '../../contracts/inventory.v1.schema.json');
const MESSAGING_SCHEMA = resolve(WEB, '../../contracts/messaging.v1.schema.json');
const NEW_THREAD_SCHEMA = resolve(WEB, '../../contracts/new-thread.v1.schema.json');
// The JSON validators: the only files that may assert types or use arbitrary string keys.
const VALIDATORS = new Set(['src/envelope.ts', 'src/messaging.ts', 'src/newthread.ts']);
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

const posts = [];
const postCalls = [];
const threadCalls = [];
const threadPreviewCalls = [];
const dialogs = [];
const composerUses = [];
const launcherUses = [];
const promptBodyUses = [];
const textareas = [];
const trays = [];
const fileInputs = [];
const images = [];
const uploadCalls = [];
const removeCalls = [];
const trayHooks = [];
const objectUrls = [];

/**
 * Whether `node` runs only inside the owner's own event. A check: a JSX
 * `on…` attribute, or a `const on… =` handler. A send : only a
 * `<SendControl onPress>` (the one way a send button is rendered), the
 * keyboard's `const onKeyDown`, or the `const send` helper they call.
 */
function fromOwnerEvent(node, sending) {
  for (let n = node.parent; n; n = n.parent) {
    if (ts.isJsxAttribute(n)) {
      const name = n.name.getText();
      if (!sending) return /^on[A-Z]/.test(name);
      return name === 'onPress' && n.parent.parent.tagName?.getText() === 'SendControl';
    }
    if (ts.isVariableDeclaration(n) && ts.isIdentifier(n.name)) return sending ? /^(onKeyDown|send)$/.test(n.name.text) : /^(on[A-Z]\w*|send)$/.test(n.name.text);
    if (ts.isCallExpression(n) && /^(use\w*Effect|setTimeout|setInterval|requestAnimationFrame|queueMicrotask)$/.test(n.expression.getText().replace(/^(window|globalThis)\./, ''))) return false;
    if (ts.isFunctionDeclaration(n) || ts.isMethodDeclaration(n) || ts.isClassDeclaration(n) || ts.isSourceFile(n)) return false;
  }
  return false;
}

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
        } else if (!numeric && !VALIDATORS.has(rel)) problem(file, node, 'a computed member access with a string key');
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
    // Function(...) without new, globalThis.Function, and the constructor route to it .
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
      // the check: the composer keeps an unconfirmed send in this tab's sessionStorage, read in one place.
      const storage = node.text === 'sessionStorage' && rel === 'src/composer.ts' && node.parent.getText() === 'window.sessionStorage';

      const xhr = node.text === 'XMLHttpRequest' && rel === 'src/api.ts' && ts.isNewExpression(node.parent) && node.parent.expression === node;
      if (!storage && !xhr) problem(file, node, `a banned browser API: ${node.text}`);
    }
    // W5: the one POST, in src/api.ts.
    if (ts.isStringLiteralLike(node) && node.text.toUpperCase() === 'POST') {
      if (rel !== 'src/api.ts') problem(file, node, 'a POST outside src/api.ts');
      else posts.push(node);
    }
    // W5: the send body's flags are set only where the body is built.
    if (ts.isPropertyAssignment(node) && ['resume_paused_queue', 'accept_possible_duplicate'].includes(node.name.getText()) && rel !== 'src/composer.ts') {
      problem(file, node, `the send body built outside src/composer.ts: ${node.name.getText()}`);
    }

    if (ts.isPropertyAssignment(node) && node.name.getText() === 'attachments' && rel !== 'src/launcher.ts' && rel !== 'src/attachments.ts' && !VALIDATORS.has(rel)) {
      problem(file, node, 'the create body built outside src/launcher.ts: attachments');
    }

    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isPropertyAccessExpression(node.left) && node.left.name.text === 'attachments' && rel !== 'src/composer.ts') {
      problem(file, node, 'the send body built outside src/composer.ts: attachments');
    }
    if (ts.isCallExpression(node)) {
      const callee = node.expression;
      const name = ts.isIdentifier(callee) ? callee.text : ts.isPropertyAccessExpression(callee) ? callee.name.text : null;
      if (name === 'postMessage') {
        if (rel !== 'src/composer.ts' || !ts.isIdentifier(callee)) problem(file, node, 'postMessage called outside src/composer.ts');
        else postCalls.push(node);
      }

      if (name === 'postThread' || name === 'postThreadPreview') {
        if (rel !== 'src/launcher.ts' || !ts.isIdentifier(callee)) problem(file, node, `${name} called outside src/launcher.ts`);
        else (name === 'postThread' ? threadCalls : threadPreviewCalls).push(node);
      }

      if (name === 'postAttachment' || name === 'deleteAttachment') {
        if (rel !== 'src/attachments.ts' || !ts.isIdentifier(callee)) problem(file, node, `${name} called outside src/attachments.ts`);
        else (name === 'postAttachment' ? uploadCalls : removeCalls).push(node);
      }
      // AW7: object URLs (the owner's own file, for a thumbnail) are made in one place.
      if (name === 'createObjectURL') objectUrls.push(rel);
      // AW8: in the components, an upload, a retry or a removal starts only inside the owner's own event handler.
      if (ts.isPropertyAccessExpression(callee) && ['add', 'retry', 'remove'].includes(callee.name.text) && /uploads$/.test(callee.expression.getText()) && rel.startsWith('src/components/') && !fromOwnerEvent(node, false)) {
        problem(file, node, `an upload, retry or removal outside an owner's event handler: ${callee.getText()}`);
      }
      // U12, U13: in the components, a create starts only inside the owner's own event handler.
      if (ts.isPropertyAccessExpression(callee) && callee.name.text === 'create' && rel.startsWith('src/components/') && !fromOwnerEvent(node, false)) {
        problem(file, node, `a create outside an owner's event handler: ${callee.getText()}`);
      }
      // W5, W11: in the components, a send (`.press`, or the `send` helper)
      // or a check (`.check`) starts only inside the owner's own event
      // handler: a JSX `on…` attribute, or a `const on… =` handler. Never in
      // an effect, a timer or a render.
      const sending = (ts.isPropertyAccessExpression(callee) && callee.name.text === 'press') || (ts.isIdentifier(callee) && callee.text === 'send');
      const checking = ts.isPropertyAccessExpression(callee) && callee.name.text === 'check';
      if ((sending || checking) && rel.startsWith('src/components/') && !fromOwnerEvent(node, sending)) {
        problem(file, node, `a ${sending ? 'send outside a <SendControl onPress> or the keyboard handler' : 'check outside an owner\'s event handler'}: ${callee.getText()}`);
      }
      // The press of a send button: only inside SendControl, the one way a send button is rendered.
      if (ts.isIdentifier(callee) && callee.text === 'onPress') {
        let fn = node.parent;
        while (fn && !ts.isFunctionDeclaration(fn)) fn = fn.parent;
        if (fn?.name?.text !== 'SendControl') problem(file, node, 'onPress called outside SendControl');
      }
    }
    // W12: one panel and sheet, and the composer only inside it.
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = node.tagName.getText();
      if (tag === 'dialog') dialogs.push(`${rel}`);
      if (tag === 'Composer') composerUses.push(`${rel}`);
      if (tag === 'Launcher') launcherUses.push(`${rel}`);
      if (tag === 'PromptBody') promptBodyUses.push(`${rel}`);
      if (tag === 'textarea') textareas.push(`${rel}`);
      if (tag === 'AttachmentTray') trays.push(`${rel}`);
      if (tag === 'img') images.push(`${rel}`);
      if (['object', 'embed', 'iframe', 'frame', 'portal'].includes(tag)) problem(file, node, `<${tag}>: no embedded content`);
      const typeAttr = node.attributes.properties.find((a) => a.name?.getText() === 'type');
      if (tag === 'input' && typeAttr?.initializer && ts.isStringLiteral(typeAttr.initializer) && typeAttr.initializer.text === 'file') fileInputs.push(`${rel}`);
      const props = node.attributes.properties.map((a) => a.name?.getText());
      if (props.includes('onPress') && tag !== 'SendControl') problem(file, node, `onPress on <${tag}>: only a SendControl sends`);
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
    // AW8: one tray hook.
    if (ts.isFunctionDeclaration(node) && node.name?.text === 'useTray') trayHooks.push(rel);
    // Types.
    if (node.kind === ts.SyntaxKind.AnyKeyword) problem(file, node, 'the any type');
    if ((ts.isAsExpression(node) || ts.isTypeAssertionExpression(node)) && !VALIDATORS.has(rel)) {
      if (!(ts.isAsExpression(node) && node.type.getText() === 'const')) problem(file, node, 'a type assertion outside the JSON validators');
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

// W5: exactly one POST and one place that sends.
if (posts.length !== 1) problems.push(`src/api.ts: ${posts.length} POST literals; exactly one (postMessage) is allowed`);
if (postCalls.length !== 1) problems.push(`src/composer.ts: postMessage is called ${postCalls.length} times; exactly once (Composers.press) is allowed`);

// W12: one panel and sheet, the composer only inside it, and no new page.
if (dialogs.join() !== 'src/components/Detail.tsx') problems.push(`W12: <dialog> in ${dialogs.join(', ') || 'no file'}; exactly one, in src/components/Detail.tsx`);
if (composerUses.join() !== 'src/components/Detail.tsx') problems.push(`W12: <Composer> in ${composerUses.join(', ') || 'no file'}; exactly once, in src/components/Detail.tsx`);
// U14 : the launcher only in the one dialog, and one prompt body.
if (launcherUses.join() !== 'src/components/Detail.tsx') problems.push(`U14: <Launcher> in ${launcherUses.join(', ') || 'no file'}; exactly once, in src/components/Detail.tsx`);
if ([...promptBodyUses].sort().join() !== 'src/components/Composer.tsx,src/components/Launcher.tsx') {
  problems.push(`U14: <PromptBody> in ${promptBodyUses.join(', ') || 'no file'}; exactly once each in src/components/Composer.tsx and src/components/Launcher.tsx`);
}
if (textareas.join() !== 'src/components/PromptBody.tsx') problems.push(`U14: <textarea> in ${textareas.join(', ') || 'no file'}; exactly once, in src/components/PromptBody.tsx (the one prompt body)`);
// U12 : one create and one preview call, each in one place.
if (threadCalls.length !== 1) problems.push(`src/launcher.ts: postThread is called ${threadCalls.length} times; exactly once (Launchers.create) is allowed`);
if (threadPreviewCalls.length !== 1) problems.push(`src/launcher.ts: postThreadPreview is called ${threadPreviewCalls.length} times; exactly once (Launchers.preview) is allowed`);
// AW7, AW8 : one tray, one file input, one hook, one upload and one removal call; images and object URLs in one place each.
if (trays.join() !== 'src/components/PromptBody.tsx') problems.push(`AW8: <AttachmentTray> in ${trays.join(', ') || 'no file'}; exactly once, in src/components/PromptBody.tsx`);
if (fileInputs.join() !== 'src/components/AttachmentTray.tsx') problems.push(`AW8: a file input in ${fileInputs.join(', ') || 'no file'}; exactly once, in src/components/AttachmentTray.tsx`);
if (trayHooks.join() !== 'src/components/AttachmentTray.tsx') problems.push(`AW8: useTray in ${trayHooks.join(', ') || 'no file'}; exactly once, in src/components/AttachmentTray.tsx`);
if (uploadCalls.length !== 1) problems.push(`src/attachments.ts: postAttachment is called ${uploadCalls.length} times; exactly once (Uploads.upload) is allowed`);
if (removeCalls.length !== 1) problems.push(`src/attachments.ts: deleteAttachment is called ${removeCalls.length} times; exactly once (Uploads.remove) is allowed`);
if (images.join() !== 'src/components/AttachmentTray.tsx') problems.push(`AW7: <img> in ${images.join(', ') || 'no file'}; exactly once, in src/components/AttachmentTray.tsx (a thumbnail of the owner's own file)`);
if (objectUrls.join() !== 'src/components/App.tsx') problems.push(`AW7: createObjectURL in ${objectUrls.join(', ') || 'no file'}; exactly once, in src/components/App.tsx`);
const PAGES = ['404.tsx', '_app.tsx', '_document.tsx', 'index.tsx'];
const pages = walk(join(WEB, 'pages')).map((f) => relative(join(WEB, 'pages'), f)).sort();
if (pages.join() !== PAGES.join()) problems.push(`W12: pages/ has ${pages.join(', ')}; only ${PAGES.join(', ')}`);

// W5: the messaging types against contracts/messaging.v1.schema.json, for every definition the web reads.
const mschema = JSON.parse(readFileSync(MESSAGING_SCHEMA, 'utf8'));
const mdefs = mschema.$defs;
const msf = program.getSourceFile(join(WEB, 'src/messaging.ts'));
if (!msf) problems.push('src/messaging.ts: missing (the send shapes the composer reads)');
const maliases = new Map();
if (msf) {
  ts.forEachChild(msf, (n) => {
    if (ts.isTypeAliasDeclaration(n)) maliases.set(n.name.text, n);
  });
}
const mprops = (def, seen = new Set()) => {
  if (def.$ref) {
    const name = def.$ref.replace('#/$defs/', '');
    if (seen.has(name)) return new Set();
    seen.add(name);
    return mprops(mdefs[name], seen);
  }
  const out = new Set(Object.keys(def.properties ?? {}));
  for (const branch of def.oneOf ?? []) for (const p of mprops(branch, seen)) out.add(p);
  return out;
};
const mtsProps = (typeNode, seen = new Set()) => {
  const out = new Set();
  if (!typeNode) return out;
  if (ts.isTypeLiteralNode(typeNode)) {
    for (const m of typeNode.members) if (m.name) out.add(m.name.getText().replace(/['"]/g, ''));
  } else if (ts.isUnionTypeNode(typeNode) || ts.isIntersectionTypeNode(typeNode)) {
    for (const t of typeNode.types) for (const p of mtsProps(t, seen)) out.add(p);
  } else if (ts.isTypeReferenceNode(typeNode)) {
    const name = typeNode.typeName.getText();
    if (maliases.has(name) && !seen.has(name)) {
      seen.add(name);
      for (const p of mtsProps(maliases.get(name).type, seen)) out.add(p);
    }
  }
  return out;
};
const READ = ['report', 'preview', 'refusal', 'lookup', 'target', 'source', 'gap', 'effects', 'effect', 'paused_queue', 'result', 'earlier_send', 'delivery', 'reconciliation'];
for (const name of READ) {
  const alias = maliases.get(pascal(name));
  if (!alias) {
    problems.push(`src/messaging.ts: no type ${pascal(name)} for the schema's ${name}`);
    continue;
  }
  const want = [...mprops(mdefs[name])].sort().join(', ');
  const have = [...mtsProps(alias.type)].sort().join(', ');
  if (want !== have) problems.push(`src/messaging.ts: ${pascal(name)} has {${have}}, the schema's ${name} has {${want}}`);
}
const failReasons = msf?.text.match(/FAIL_REASONS = \[([\s\S]*?)\]/)?.[1].match(/'([a-z_]+)'/g)?.map((s) => s.slice(1, -1)) ?? [];
if (failReasons.join() !== mdefs.fail_reason.enum.join()) problems.push("src/messaging.ts: FAIL_REASONS differs from the schema's fail_reason");
for (const [id, def] of [['REPORT_SCHEMA_ID', mdefs.report], ['PREVIEW_SCHEMA_ID', mdefs.preview], ['LOOKUP_SCHEMA_ID', mdefs.lookup]]) {
  if (!msf?.text.includes(`${id} = '${def.properties.schema.const}'`)) problems.push(`src/messaging.ts: ${id} differs from the schema`);
}

// U5 : the new-thread types against contracts/new-thread.v1.schema.json, for every definition the launcher reads.
const nschema = JSON.parse(readFileSync(NEW_THREAD_SCHEMA, 'utf8'));
const ndefs = nschema.$defs;
const nsf = program.getSourceFile(join(WEB, 'src/newthread.ts'));
if (!nsf) problems.push('src/newthread.ts: missing (the new-thread shapes the launcher reads)');
const naliases = new Map();
if (nsf) {
  ts.forEachChild(nsf, (n) => {
    if (ts.isTypeAliasDeclaration(n)) naliases.set(n.name.text, n);
  });
}
const nprops = (def, seen = new Set()) => {
  if (def.$ref) {
    const name = def.$ref.replace('#/$defs/', '');
    if (seen.has(name)) return new Set();
    seen.add(name);
    return nprops(ndefs[name], seen);
  }
  const out = new Set(Object.keys(def.properties ?? {}));
  for (const branch of def.oneOf ?? []) for (const p of nprops(branch, seen)) out.add(p);
  return out;
};
const ntsProps = (typeNode, seen = new Set()) => {
  const out = new Set();
  if (!typeNode) return out;
  if (ts.isTypeLiteralNode(typeNode)) {
    for (const m of typeNode.members) if (m.name) out.add(m.name.getText().replace(/['"]/g, ''));
  } else if (ts.isUnionTypeNode(typeNode) || ts.isIntersectionTypeNode(typeNode)) {
    for (const t of typeNode.types) for (const p of ntsProps(t, seen)) out.add(p);
  } else if (ts.isTypeReferenceNode(typeNode)) {
    const name = typeNode.typeName.getText();
    if (naliases.has(name) && !seen.has(name)) {
      seen.add(name);
      for (const p of ntsProps(naliases.get(name).type, seen)) out.add(p);
    }
  }
  return out;
};
const NREAD = [
  'options',
  'harness_option',
  'model_option',
  'unavailable_harness',
  'attachment_support',
  'preview',
  'report',
  'result',
  'outcome',
  'effects',
  'effect',
  'project_view',
  'project_ref',
  'source',
  'settings',
  'gap',
  'start_view',
  'start_stage',
  'reported_configuration',
  'reported_setting',
  'status_read',
  'observations',
  'observed_thread',
];
for (const name of NREAD) {
  const alias = naliases.get(pascal(name));
  if (!alias) {
    problems.push(`src/newthread.ts: no type ${pascal(name)} for the schema's ${name}`);
    continue;
  }
  const want = [...nprops(ndefs[name])].sort().join(', ');
  const have = [...ntsProps(alias.type)].sort().join(', ');
  if (want !== have) problems.push(`src/newthread.ts: ${pascal(name)} has {${have}}, the schema's ${name} has {${want}}`);
}
for (const [list, def] of [
  ['EFFECT_KINDS', ndefs.effect.properties.kind],
  ['REFUSAL_REASONS', ndefs.refusal_reason],
  ['FAIL_REASONS', ndefs.fail_reason],
  ['UNCERTAIN_REASONS', ndefs.uncertain_reason],
]) {
  const have = nsf?.text.match(new RegExp(`${list} = \\[([\\s\\S]*?)\\]`))?.[1].match(/'([a-z_]+)'/g)?.map((x) => x.slice(1, -1)) ?? [];
  if (have.join() !== def.enum.join()) problems.push(`src/newthread.ts: ${list} differs from the schema`);
}
for (const [id, def] of [['OPTIONS_SCHEMA_ID', ndefs.options], ['PREVIEW_SCHEMA_ID', ndefs.preview], ['REPORT_SCHEMA_ID', ndefs.report], ['STATUS_SCHEMA_ID', ndefs.status_read]]) {
  if (!nsf?.text.includes(`${id} = '${def.properties.schema.const}'`)) problems.push(`src/newthread.ts: ${id} differs from the schema`);
}

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
console.log(`check: ${files.length} source files and the stylesheet pass (V1, messaging and new-thread schema parity, V3, V10, W5, W12, U5, U12 to U14, AW7, AW8, types, spacing)`);
