// The new-thread fixtures preserve effect kinds and use machine-neutral text.
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parsePreview, parseReport } from '../../src/newthread.ts';
import * as nt from '../support/newthread-synthetic.ts';

const body = { op_id: 'syn-1', text: 'synthetic', attachments: [], harness: 'codex', model: 'syn-model', effort: 'low' };

for (const [label, value, parse] of [
  ['preview', nt.preview('prj_synthetic', body), parsePreview],
  ['report', nt.report('prj_synthetic', body, nt.OUTCOMES.created('thr_synthetic')), parseReport],
] as const) {
  test(`${label} fixture uses machine-neutral effect text with unchanged kinds`, () => {
    const parsed = parse(value);
    assert.ok(parsed, 'the fixture passes the public parser');
    assert.deepEqual(
      parsed.effects?.effects.filter((effect) => effect.kind === 'copies_env_files' || effect.kind === 'mac_stays_awake'),
      [
        { kind: 'copies_env_files', text: "copies the project's .env files from this computer" },
        { kind: 'mac_stays_awake', text: 'this computer must stay awake until the start is confirmed' },
      ],
    );
  });
}
