// The real openwiz-server, run over one of the fixture scenarios with the
// repository's fake room-cli first on PATH. Synthetic data only.

import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { REPO_DIR, SCENARIOS_DIR } from './paths.mjs';

const SERVER = `${REPO_DIR}/target/debug/openwiz-server`;
const FAKE = `${REPO_DIR}/target/debug/fake-room-cli`;

let built = false;

/** Builds the server and the fake once per test process (a no-op when current). */
export function buildBinaries() {
  if (built) return;
  execFileSync('cargo', ['build', '-q', '-p', 'openwiz-server', '-p', 'openwiz-tests', '--bin', 'openwiz-server', '--bin', 'fake-room-cli'], {
    cwd: REPO_DIR,
    stdio: 'inherit',
  });
  built = true;
}

/** Every 0.0.1 scenario that has a scenario.json. */
export function scenarios() {
  return readdirSync(SCENARIOS_DIR)
    .filter((name) => existsSync(join(SCENARIOS_DIR, name, 'scenario.json')))
    .sort();
}

/**
 * Starts openwiz-server on a free loopback port over `scenario`. With
 * `allowedHost`, it also takes that host name, which is what lets
 * it send; its send record is a private directory.
 */
export async function startFixtureServer(scenario, { allowedHost = null, responses = null } = {}) {
  buildBinaries();
  const dir = mkdtempSync(join(tmpdir(), 'openwiz-web-'));
  symlinkSync(FAKE, join(dir, 'room-cli'));
  let file = join(SCENARIOS_DIR, scenario, 'scenario.json');
  if (responses) {
    // The test's own responses, tried before `scenario`'s, as tests/messaging's override does. Synthetic.
    mkdirSync(join(dir, 'own'));
    file = join(dir, 'own', 'scenario.json');
    const own = { scenario: 'own', description: 'A test’s own responses.', extends: join(SCENARIOS_DIR, scenario), synthetic_assumptions: ['Written by the test.'], responses };
    writeFileSync(file, JSON.stringify(own, null, 2));
  }
  writeFileSync(join(dir, 'fake-room-cli.scenario'), file);
  const args = ['--port', '0', ...(allowedHost ? ['--allowed-host', allowedHost] : [])];
  const child = spawn(SERVER, args, {
    cwd: dir,
    env: { PATH: `${dir}:/usr/bin:/bin`, HOME: dir, OPENWIZ_STATE_DIR: join(dir, 'state') },
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  let stderr = '';
  const port = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`openwiz-server did not start: ${stderr}`)), 15_000);
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
      const m = /listening on http:\/\/127\.0\.0\.1:(\d+)/.exec(stderr);
      if (m) {
        clearTimeout(timer);
        resolve(Number(m[1]));
      }
    });
    child.on('exit', (code) => reject(new Error(`openwiz-server exited ${code}: ${stderr}`)));
  });
  return {
    port,
    /** The fake's invocations, one argument list per call. */
    invocations: () => {
      const file = join(dir, 'invocations.log');
      return existsSync(file) ? readFileSync(file, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : [];
    },
    log: () => stderr,
    stop: async () => {
      child.kill('SIGTERM');
      await new Promise((resolve) => (child.exitCode !== null ? resolve() : child.on('exit', resolve)));
      // A fake that hangs ("hang": true) outlives a stopped server: stop it by its own pid.
      const hang = join(dir, 'hang.pid');
      if (existsSync(hang)) {
        try {
          process.kill(Number(readFileSync(hang, 'utf8')), 'SIGKILL');
        } catch {
          // Already reaped by the core's read timeout.
        }
      }
      rmSync(dir, { recursive: true, force: true });
    },
  };
}
