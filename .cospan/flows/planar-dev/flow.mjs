import { agent, refine } from 'cospan/flow';
import devPrompt from './dev.md';
import prPrompt from './pr.md';
import { attachPrTrace } from './pr-trace-tool.mjs';

const coderAgent = agent('agy/gemini-4-neon', { effort: 'xhigh', role: 'coder' });
const codeReviewer = agent('codex/gpt-6.2-sol', {
  effort: 'high',
  readonly: true,
  role: 'reviewer',
});
const diffSimplifier = agent('agy/gemini-4-neon', {
  effort: 'xhigh',
  readonly: true,
  role: 'simplifier',
});

export default async ({ prompt: issuePrompt, cwd, trace }) => {
  let coderSession = await coderAgent.run(`${devPrompt}\n\n## Task\n${issuePrompt}`, {
    stage: 'dev',
  });
  const unresolvedReviewNotes = [];

  coderSession = await refine(coderSession, {
    evaluator: codeReviewer,
    criteria: 'Follow REVIEW.md (and rust-review / DESIGN.md for touched surfaces).',
    maxRounds: 2,
    stage: 'review',
  });
  if (coderSession.blocked) return coderSession; // halt only on true P0 blockers
  if (!coderSession.passed) {
    unresolvedReviewNotes.push(
      `Review residual note: ${coderSession.feedback || coderSession.summary}`,
    );
  }

  coderSession = await refine(coderSession, {
    evaluator: diffSimplifier,
    criteria: 'Follow SIMPLIFY.md.',
    maxRounds: 1,
    stage: 'simplify',
  });
  if (coderSession.blocked) return coderSession;
  if (!coderSession.passed) {
    unresolvedReviewNotes.push(
      `Simplification note: ${coderSession.feedback || coderSession.summary}`,
    );
  }

  const residualNotesSection = unresolvedReviewNotes.length
    ? `\n\n## Residual Non-Blocking Notes (mention transparently in the PR body)\n- ${unresolvedReviewNotes.join('\n- ')}`
    : '';
  const openPrPrompt = `${prPrompt}${residualNotesSection}`;

  const publishedSession = await coderSession.continue(openPrPrompt, { stage: 'pr' });
  attachPrTrace(cwd, trace);
  return publishedSession;
};
