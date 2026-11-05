
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { request } from 'node:http';
import { startFixtureServer } from '../../apps/web/test/support/rust.mjs';
import { parseReport, parseLookup } from '../../apps/web/src/messaging.ts';
import { edit, EMPTY, press, sendAnswer, settle } from '../../apps/web/src/composer.ts';
const host = 'openwiz.example.test';
const id = 'thr_syn_0006';
const payload = JSON.parse(readFileSync(new URL('../fixtures/room-cli-0.43.1/send/tell-cloud-queue-observed.json', import.meta.url)));
payload.threadId = id;
payload.queuedMessage.threadId = id;
const draft = edit(EMPTY, 'Synthetic Cloud Queue proof', () => 'web-00000000000000000000000000001007');
const attempt = press(draft, { mode: 'queue', resumePausedQueue: false, acceptPossibleDuplicate: false });
assert.ok(attempt);
const server = await startFixtureServer('send', { allowedHost: host, responses: [{ argv: ['thread', 'tell', id, '--mode', 'queue', '--json', '--'], any_text: true, stdout: JSON.stringify(payload) }] });
function call(method, path, data) {
  const body = data ? JSON.stringify(data) : '';
  return new Promise((resolve, reject) => {
    const req = request({ hostname: '127.0.0.1', port: server.port, method, path, headers: { Host: host, Origin: `https://${host}`, 'Sec-Fetch-Site': 'same-origin', 'X-OpenWiz-Send': '1', 'X-Requested-With': 'XMLHttpRequest', 'Content-Type': 'application/json', ...(body ? { 'Content-Length': Buffer.byteLength(body) } : {}) } }, (res) => {
      let text = '';
      res.setEncoding('utf8');
      res.on('data', chunk => text += chunk);
      res.on('end', () => resolve({ status: res.statusCode, json: JSON.parse(text) }));
    });
    req.setTimeout(10_000, () => req.destroy(new Error('fixture HTTP request timed out')));
    req.on('error', reject);
    req.end(body);
  });
}
try {
  const path = `/api/v1/sessions/${id}/messages`;
  const body = { op_id: attempt.opId, mode: 'queue', text: draft.draft };
  for (let n = 0; n < 2; n++) {
    const answer = await call('POST', path, body);
    assert.equal(answer.status, 200);
    const report = parseReport(answer.json);
    assert.ok(report);
    assert.equal(report.status, 'queued');
    assert.equal(report.requested_mode, 'queue');
    assert.equal(report.result.delivery.waiting_on, 'not_reported');
    assert.equal(report.result.delivery.queued_message_id, '00000000-0000-4000-8000-000000000001');
    assert.equal(report.result.delivery.text, 'queued: waiting reason not reported');
    assert.equal(report.result.replayed, n === 1);
    const state = settle(attempt.state, attempt.opId, sendAnswer({ kind: 'response', status: 200, json: report, empty: false }, id, attempt.opId));
    assert.equal(state.attempts[0].state, 'done');
    assert.equal(state.attempts[0].result, 'queued');
    assert.equal(state.draft, '');
  }
  const lookup = await call('GET', `${path}/${attempt.opId}`);
  assert.equal(lookup.status, 200);
  const parsed = parseLookup(lookup.json);
  assert.ok(parsed);
  assert.equal(parsed.result.kind, 'recorded');
  assert.equal(parsed.result.report.status, 'queued');
  assert.equal(parsed.result.report.result.delivery.waiting_on, 'not_reported');
  assert.equal(server.invocations().filter(a => a[0] === 'thread' && a[1] === 'tell').length, 1);
  console.log('PASS #1007: fixture HTTP send/replay/lookup -> web parser/composer queued, waiting reason not reported, UUID retained, draft cleared, exactly one fake tell.');
} finally {
  await server.stop();
}
