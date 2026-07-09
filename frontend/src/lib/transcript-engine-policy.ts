/**
 * Policy: which transcription engine/model is best for a given language.
 *
 * Rationale:
 * - **Parakeet** is fast and highly accurate, but only its English path is
 *   rock-solid across the board; it does not handle non-European scripts at all
 *   (Persian, Arabic, Hindi, CJK, Thai, Hebrew, ...).
 * - **Whisper** is the multilingual accuracy engine and the only local option
 *   that transcribes those languages.
 *
 * So: English → Parakeet, every other language → Whisper. This matches the
 * common case (fast realtime English, accurate multilingual everything-else)
 * and never routes a language to an engine that can't produce it.
 *
 * To let Parakeet handle more languages later (its v3 model supports ~25
 * European languages), just add their ISO codes to PARAKEET_PREFERRED.
 */

export type Engine = 'whisper' | 'parakeet';

/** ISO 639-1 codes for which Parakeet is the preferred engine. */
export const PARAKEET_PREFERRED: ReadonlySet<string> = new Set(['en']);

/**
 * Cloud providers that transcribe every language well (e.g. ElevenLabs Scribe
 * covers 90+ languages). When one of these is the active provider, switching
 * the transcription language must never yank the user back to a local engine —
 * they opted into the cloud provider explicitly.
 */
export const MULTILINGUAL_CLOUD_PROVIDERS: ReadonlySet<string> = new Set([
  'elevenLabs',
  'deepgram',
  'groq',
  'openai',
]);

export function isMultilingualCloudProvider(provider: string | undefined): boolean {
  return !!provider && MULTILINGUAL_CLOUD_PROVIDERS.has(provider);
}

/**
 * Preferred engine for a language code, or null for the auto-detect modes
 * (where we deliberately leave the user's engine choice untouched).
 */
export function preferredEngineForLanguage(code: string): Engine | null {
  if (!code || code === 'auto' || code === 'auto-translate') return null;
  return PARAKEET_PREFERRED.has(code) ? 'parakeet' : 'whisper';
}

// Best-first ranking of known model names per engine.
const WHISPER_RANK = [
  'large-v3',
  'large-v3-turbo',
  'large-v3-q5_0',
  'large-v3-turbo-q5_0',
  'medium',
  'medium-q5_0',
  'small',
  'small-q5_1',
  'base',
  'base-q5_1',
  'tiny',
  'tiny-q5_1',
];

const PARAKEET_RANK = ['parakeet-tdt-0.6b-v3-int8', 'parakeet-tdt-0.6b-v2-int8'];

export interface AvailableModel {
  provider: Engine;
  name: string;
}

/**
 * Best downloaded model for the given engine, or null if none is available.
 * Prefers larger/newer models; falls back to any present model of that engine
 * whose name we don't recognise.
 */
export function bestAvailableModel(
  engine: Engine,
  available: AvailableModel[],
): string | null {
  const rank = engine === 'whisper' ? WHISPER_RANK : PARAKEET_RANK;
  const names = available.filter((m) => m.provider === engine).map((m) => m.name);
  for (const r of rank) {
    if (names.includes(r)) return r;
  }
  return names[0] ?? null;
}

/** Map an engine to the provider string used by the transcript config / backend. */
export function engineToProvider(engine: Engine): 'localWhisper' | 'parakeet' {
  return engine === 'whisper' ? 'localWhisper' : 'parakeet';
}

/** Map a stored provider string back to an engine ('localWhisper' | 'whisper' → whisper). */
export function providerToEngine(provider: string | undefined): Engine | null {
  if (provider === 'parakeet') return 'parakeet';
  if (provider === 'localWhisper' || provider === 'whisper') return 'whisper';
  return null;
}
