import { execFileSync } from 'node:child_process';

export function attachPrTrace(cwd, trace) {
  if (!trace?.upsertIn) return;
  try {
    const prJson = execFileSync('gh', ['pr', 'view', '--json', 'number,body'], {
      cwd,
      encoding: 'utf8',
    });
    const { number, body = '' } = JSON.parse(prJson);
    if (!number) return;
    execFileSync('gh', ['pr', 'edit', String(number), '--body-file', '-'], {
      cwd,
      input: `${trace.upsertIn(body)}\n`,
      stdio: ['pipe', 'ignore', 'ignore'],
    });
  } catch {
    // Best-effort telemetry: never fail a completed PR over trace attachment.
  }
}
