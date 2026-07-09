'use client';

import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { useConfig } from '@/contexts/ConfigContext';
import { LANGUAGES } from '@/components/LanguageSelection';
import type { RawModelInfo } from '@/hooks/useTranscriptionModels';
import {
  preferredEngineForLanguage,
  bestAvailableModel,
  engineToProvider,
  providerToEngine,
  type AvailableModel,
} from '@/lib/transcript-engine-policy';

/**
 * English ⇄ Persian transcription-language toggle for the recording pill.
 *
 * Writes through `useConfig().setSelectedLanguage`, which persists to
 * localStorage AND syncs the preference to Rust live. Because the transcription
 * worker reads the language per audio chunk, switching here takes effect
 * immediately — even mid-recording. Other languages remain available in
 * Settings → Transcript.
 */
export function QuickLanguageSwitcher() {
  const { selectedLanguage, setSelectedLanguage, transcriptModelConfig, setTranscriptModelConfig } =
    useConfig();

  const current = LANGUAGES.find((l) => l.code === selectedLanguage);

  const choose = (code: string) => {
    if (code === selectedLanguage) return;
    setSelectedLanguage(code);
    // Auto-pick the best transcription engine/model for this language.
    void autoSelectEngine(code);
  };

  /**
   * Switch the transcription engine to the one best suited to `code`
   * (English → Parakeet, Persian → ElevenLabs when a key is configured,
   * other languages → Whisper), choosing the best model already downloaded.
   * Applies to the next recording. Never switches to a local model that isn't
   * downloaded — if the preferred engine has none, it nudges the user to
   * download one instead of leaving them in a broken state.
   */
  async function autoSelectEngine(code: string) {
    // ElevenLabs routing is opt-in: only when the user has saved an API key.
    let elevenLabsReady = false;
    try {
      const key = await invoke<string>('api_get_transcript_api_key', { provider: 'elevenLabs' });
      elevenLabsReady = !!key?.trim();
    } catch {
      // No key readable — treat as not configured.
    }

    const preferred = preferredEngineForLanguage(code, { elevenLabsReady });
    if (!preferred) return; // auto / auto-translate: leave the engine untouched

    // Already on the right engine? Respect the user's model choice, do nothing.
    if (providerToEngine(transcriptModelConfig?.provider) === preferred) return;

    const langName = LANGUAGES.find((l) => l.code === code)?.name ?? code;

    if (preferred === 'elevenLabs') {
      const model = bestAvailableModel('elevenLabs', []) ?? 'scribe_v2';
      setTranscriptModelConfig((prev) => ({ ...prev, provider: 'elevenLabs', model }));
      try {
        await invoke('api_save_transcript_config', { provider: 'elevenLabs', model, apiKey: null });
      } catch (err) {
        console.error('Failed to persist transcript config:', err);
      }
      toast.success('Transcription engine set to ElevenLabs Scribe', {
        description: `Best quality for ${langName} (cloud, applies to your next recording).`,
      });
      return;
    }

    const engineLabel = preferred === 'whisper' ? 'Whisper' : 'Parakeet';

    // Gather downloaded models from both engines.
    const available: AvailableModel[] = [];
    try {
      const whisper = await invoke<RawModelInfo[]>('whisper_get_available_models');
      for (const m of whisper) {
        if (m.status === 'Available') available.push({ provider: 'whisper', name: m.name });
      }
    } catch (err) {
      console.error('Failed to list Whisper models:', err);
    }
    try {
      const parakeet = await invoke<RawModelInfo[]>('parakeet_get_available_models');
      for (const m of parakeet) {
        if (m.status === 'Available') available.push({ provider: 'parakeet', name: m.name });
      }
    } catch (err) {
      console.error('Failed to list Parakeet models:', err);
    }

    const best = bestAvailableModel(preferred, available);
    if (!best) {
      toast.info(`No ${engineLabel} model downloaded`, {
        description: `For best ${langName} transcription, download a ${engineLabel} model in Settings → Transcript.`,
      });
      return;
    }

    const provider = engineToProvider(preferred);
    setTranscriptModelConfig((prev) => ({ ...prev, provider, model: best }));
    try {
      await invoke('api_save_transcript_config', { provider, model: best, apiKey: null });
    } catch (err) {
      console.error('Failed to persist transcript config:', err);
    }
    toast.success(`Transcription engine set to ${engineLabel}`, {
      description: `Using ${best} for ${langName} (applies to your next recording).`,
    });
  }

  const segments = [
    { code: 'en', label: 'EN', title: 'Transcribe in English' },
    { code: 'fa', label: 'فا', title: 'Transcribe in Persian (فارسی)' },
  ];

  return (
    <div
      role="group"
      aria-label="Transcription language"
      title={`Transcription language: ${current?.name ?? 'Auto Detect'}. Other languages: Settings → Transcript.`}
      className="flex items-center h-9 rounded-full bg-gray-100 p-1"
    >
      {segments.map((seg) => {
        const active = selectedLanguage === seg.code;
        return (
          <button
            key={seg.code}
            type="button"
            aria-pressed={active}
            title={seg.title}
            onClick={() => choose(seg.code)}
            className={`px-3 h-7 rounded-full text-sm font-medium transition-colors ${
              active ? 'bg-white text-blue-600 shadow-sm' : 'text-gray-500 hover:text-gray-800'
            }`}
          >
            {seg.label}
          </button>
        );
      })}
    </div>
  );
}
