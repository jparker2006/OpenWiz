import { fileURLToPath } from 'node:url';

export const WEB_DIR = fileURLToPath(new URL('../../', import.meta.url)).replace(/\/$/, '');
export const REPO_DIR = fileURLToPath(new URL('../../../../', import.meta.url)).replace(/\/$/, '');
export const OUT_DIR = `${WEB_DIR}/out`;
export const SCENARIOS_DIR = `${REPO_DIR}/tests/fixtures/room-cli-0.43.1`;
export const RESULTS_DIR = `${WEB_DIR}/test-results`;
