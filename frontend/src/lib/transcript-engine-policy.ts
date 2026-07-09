/**
 * Policy: which transcription engine/model is best for a given language.
 *
 * Rationale:
 * - **Parakeet** is fast and highly accurate, but only its English path is
 *   rock-solid across the board; it does not handle non-European scripts at all
 *   (Persian, Arabic, Hindi, CJK, Thai, Hebrew, ...).
 * - **ElevenLabs Scribe** (cloud, opt-in) is dramatically better than local
 *   Whisper for low-resource languages like Persian (~3% vs ~40% WER), so when
 *   the user has configured an API key, those languages route to it.
 * - **Whisper** is the local multilingual accuracy engine and the default for
 *   every other language.
 *
 * So: English → Parakeet, ElevenLabs-preferred languages (with key) →
 * ElevenLabs, every other language → Whisper. This never routes a language to
 * an engine that can't produce it, and never routes to the cloud unless the
 * user explicitly configured a key.
 *
 * To let Parakeet handle more languages later (its v3 model supports ~25
 * European languages), just add their ISO codes to PARAKEET_PREFERRED.
 */

export type Engine = 'whisper' | 'parakeet' | 'elevenLabs';

/** ISO 639-1 codes for which Parakeet is the preferred engine. */
export const PARAKEET_PREFERRED: ReadonlySet<string> = new Set(['en']);

/**
 * ISO 639-1 codes routed to ElevenLabs Scribe when an API key is configured.
 * These are languages where local Whisper quality is too poor to be useful.
 */
export const ELEVENLABS_PREFERRED: ReadonlySet<string> = new Set(['fa']);

/**
 * Preferred engine for a language code, or null for the auto-detect modes
 * (where we deliberately leave the user's engine choice untouched).
 * `elevenLabsReady` means the user has an ElevenLabs API key configured.
 */
export function preferredEngineForLanguage(
  code: string,
  opts?: { elevenLabsReady?: boolean },
): Engine | null {
  if (!code || code === 'auto' || code === 'auto-translate') return null;
  if (PARAKEET_PREFERRED.has(code)) return 'parakeet';
  if (opts?.elevenLabsReady && ELEVENLABS_PREFERRED.has(code)) return 'elevenLabs';
  return 'whisper';
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

// Cloud models need no download; scribe_v2 is the current best.
const ELEVENLABS_RANK = ['scribe_v2', 'scribe_v1'];

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
  // Cloud engine: nothing to download, the best model is always available.
  if (engine === 'elevenLabs') return ELEVENLABS_RANK[0];

  const rank = engine === 'whisper' ? WHISPER_RANK : PARAKEET_RANK;
  const names = available.filter((m) => m.provider === engine).map((m) => m.name);
  for (const r of rank) {
    if (names.includes(r)) return r;
  }
  return names[0] ?? null;
}

/** Map an engine to the provider string used by the transcript config / backend. */
export function engineToProvider(engine: Engine): 'localWhisper' | 'parakeet' | 'elevenLabs' {
  if (engine === 'whisper') return 'localWhisper';
  return engine;
}

/** Map a stored provider string back to an engine ('localWhisper' | 'whisper' → whisper). */
export function providerToEngine(provider: string | undefined): Engine | null {
  if (provider === 'parakeet') return 'parakeet';
  if (provider === 'elevenLabs') return 'elevenLabs';
  if (provider === 'localWhisper' || provider === 'whisper') return 'whisper';
  return null;
}
