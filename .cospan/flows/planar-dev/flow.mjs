import { execFileSync } from 'node:child_process';
import { agent, refine } from 'cospan/flow';
import devPrompt from './dev.md';
import prPrompt from './pr.md';

const coder = agent('agy/gemini-4-neon', { effort: 'high', role: 'coder' });
const reviewer = agent('codex/gpt-6.2-sol', { effort: 'high', readonly: true, role: 'reviewer' });
const simplifier = agent('agy/gemini-4-neon', {
  effort: 'high',
  readonly: true,
  role: 'simplifier',
});

function attachPrTrace(cwd, trace) {
  try {
    const raw = execFileSync('gh', ['pr', 'view', '--json', 'number,body'], {
      cwd,
      encoding: 'utf8',
    });
    const { number, body = '' } = JSON.parse(raw);
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

export default async ({ task, cwd, trace }) => {
  let work = await coder(`${devPrompt}\n\n## Task\n${task}`, { stage: 'dev' });

  work = await refine(work, {
    by: reviewer,
    criteria: 'Follow REVIEW.md (and rust-review / DESIGN.md for touched surfaces).',
    maxRounds: 2,
    stage: 'review',
  });
  if (work.blocked) return work;

  const notes = [];
  if (!work.passed) notes.push(`Review residual note: ${work.feedback || work.summary}`);

  work = await refine(work, {
    by: simplifier,
    criteria: 'Follow SIMPLIFY.md.',
    maxRounds: 1,
    stage: 'simplify',
  });
  if (work.blocked) return work;
  if (!work.passed) notes.push(`Simplification note: ${work.feedback || work.summary}`);

  const residualSection = notes.length
    ? `\n\n## Residual Non-Blocking Notes (mention transparently in the PR body)\n- ${notes.join('\n- ')}`
    : '';

  const pr = await work.continue(`${prPrompt}${residualSection}`, { stage: 'pr' });
  attachPrTrace(cwd, trace);
  return pr;
};
