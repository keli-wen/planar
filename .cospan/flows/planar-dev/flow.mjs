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

export default async ({ task, trace }) => {
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
    criteria: 'Follow .agents/skills/planar-dev/references/simplify.md.',
    maxRounds: 1,
    stage: 'simplify',
  });
  if (work.blocked) return work;
  if (!work.passed) notes.push(`Simplification note: ${work.feedback || work.summary}`);

  const residualSection = notes.length
    ? `\n\n## Residual Non-Blocking Notes (mention transparently in the PR body)\n- ${notes.join('\n- ')}`
    : '';
  const traceSection = `\n\n## Agent Workflow Trace (include verbatim at the end of the PR body)\n${trace.toMarkdown()}`;

  return await work.continue(`${prPrompt}${residualSection}${traceSection}`, { stage: 'pr' });
};
