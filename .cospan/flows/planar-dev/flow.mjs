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

export default async ({ task }) => {
  let work = await coder(`${devPrompt}\n\n## Task\n${task}`, { stage: 'dev' });

  work = await refine(work, {
    by: reviewer,
    criteria: 'Follow REVIEW.md (and rust-review / DESIGN.md for touched surfaces).',
    maxRounds: 2,
    stage: 'review',
  });
  if (!work.passed) return work;

  work = await refine(work, {
    by: simplifier,
    criteria: 'Follow .agents/skills/planar-dev/references/simplify.md.',
    maxRounds: 1,
    stage: 'simplify',
  });

  return await work.continue(prPrompt, { stage: 'pr' });
};
