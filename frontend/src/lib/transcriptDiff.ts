/**
 * Word-level diff between an original transcript segment and the user's
 * corrected version, used to learn dictionary entries automatically.
 *
 * Returns replacement pairs: contiguous runs of words that changed, e.g.
 *   original: "we use genghis gpt for triage"
 *   edited:   "we use ChatGPT for triage"
 *   -> [{ misheard: "genghis gpt", correct: "ChatGPT" }]
 */

export interface CorrectionPair {
  misheard: string;
  correct: string;
}

// Longest common subsequence table over word arrays
function lcsTable(a: string[], b: string[]): number[][] {
  const table: number[][] = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0)
  );
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      table[i][j] =
        a[i].toLowerCase() === b[j].toLowerCase()
          ? table[i + 1][j + 1] + 1
          : Math.max(table[i + 1][j], table[i][j + 1]);
    }
  }
  return table;
}

function stripPunctuation(phrase: string): string {
  // Trim leading/trailing punctuation but keep inner characters (hyphens,
  // apostrophes, Persian text) intact.
  return phrase.replace(/^[\s.,;:!?"'«»()[\]]+|[\s.,;:!?"'«»()[\]]+$/gu, '');
}

// Replacement pairs longer than this are sentence rewrites, not vocabulary
// fixes — learning them as dictionary entries would cause bad substitutions.
const MAX_PHRASE_WORDS = 4;

export function extractCorrections(original: string, edited: string): CorrectionPair[] {
  const a = original.split(/\s+/).filter(Boolean);
  const b = edited.split(/\s+/).filter(Boolean);
  if (a.length === 0 || b.length === 0) return [];

  const table = lcsTable(a, b);
  const pairs: CorrectionPair[] = [];

  let i = 0;
  let j = 0;
  let removed: string[] = [];
  let added: string[] = [];

  const flush = () => {
    // Only replacements (words on both sides) are corrections; pure
    // insertions/deletions carry no misheard->correct mapping.
    if (removed.length > 0 && added.length > 0) {
      const misheard = stripPunctuation(removed.join(' '));
      const correct = stripPunctuation(added.join(' '));
      if (
        misheard &&
        correct &&
        misheard.toLowerCase() !== correct.toLowerCase() &&
        removed.length <= MAX_PHRASE_WORDS &&
        added.length <= MAX_PHRASE_WORDS
      ) {
        pairs.push({ misheard, correct });
      }
    }
    removed = [];
    added = [];
  };

  while (i < a.length && j < b.length) {
    if (a[i].toLowerCase() === b[j].toLowerCase()) {
      flush();
      i++;
      j++;
    } else if (table[i + 1][j] >= table[i][j + 1]) {
      removed.push(a[i]);
      i++;
    } else {
      added.push(b[j]);
      j++;
    }
  }
  removed.push(...a.slice(i));
  added.push(...b.slice(j));
  flush();

  return pairs;
}
