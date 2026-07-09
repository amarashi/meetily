'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Globe, Check, ChevronDown } from 'lucide-react';
import { useConfig } from '@/contexts/ConfigContext';
import { LANGUAGES } from '@/components/LanguageSelection';
import type { RawModelInfo } from '@/hooks/useTranscriptionModels';
import {
  preferredEngineForLanguage,
  bestAvailableModel,
  engineToProvider,
  providerToEngine,
  isMultilingualCloudProvider,
  type AvailableModel,
} from '@/lib/transcript-engine-policy';

/**
 * Compact transcription-language switcher for the recording pill.
 *
 * Writes through `useConfig().setSelectedLanguage`, which persists to
 * localStorage AND syncs the preference to Rust live. Because the transcription
 * worker reads the language per audio chunk, switching here takes effect
 * immediately — even mid-recording.
 */
export function QuickLanguageSwitcher() {
  const { selectedLanguage, setSelectedLanguage, transcriptModelConfig, setTranscriptModelConfig } =
    useConfig();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Parakeet ignores the language hint; only Whisper honours it.
  const isParakeet = transcriptModelConfig?.provider === 'parakeet';

  useEffect(() => {
    if (open) inputRef.current?.focus();
    else setQuery('');
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (!containerRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDocClick);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDocClick);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const current = LANGUAGES.find((l) => l.code === selectedLanguage);

  // Short label for the button: "Auto", "→EN" for translate, else the code.
  const buttonLabel =
    selectedLanguage === 'auto' || !selectedLanguage
      ? 'Auto'
      : selectedLanguage === 'auto-translate'
        ? '→EN'
        : selectedLanguage.toUpperCase();

  const filter = query.trim().toLowerCase();
  const filtered = useMemo(() => {
    if (!filter) return LANGUAGES;
    return LANGUAGES.filter(
      (l) =>
        l.code.toLowerCase().includes(filter) ||
        l.name.toLowerCase().includes(filter),
    );
  }, [filter]);

  const choose = (code: string) => {
    setSelectedLanguage(code);
    setOpen(false);
    // Auto-pick the best transcription engine/model for this language.
    void autoSelectEngine(code);
  };

  /**
   * Switch the transcription engine to the one best suited to `code`
   * (English → Parakeet, other languages → Whisper), choosing the best model
   * already downloaded. Applies to the next recording. Never switches to a
   * model that isn't downloaded — if the preferred engine has none, it nudges
   * the user to download one instead of leaving them in a broken state.
   */
  async function autoSelectEngine(code: string) {
    // A multilingual cloud provider (e.g. ElevenLabs Scribe) handles every
    // language — the user chose it deliberately, so never override it here.
    if (isMultilingualCloudProvider(transcriptModelConfig?.provider)) return;

    const preferred = preferredEngineForLanguage(code);
    if (!preferred) return; // auto / auto-translate: leave the engine untouched

    // Already on the right engine? Respect the user's model choice, do nothing.
    if (providerToEngine(transcriptModelConfig?.provider) === preferred) return;

    const engineLabel = preferred === 'whisper' ? 'Whisper' : 'Parakeet';
    const langName = LANGUAGES.find((l) => l.code === code)?.name ?? code;

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

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        title={`Transcription language: ${current?.name ?? 'Auto Detect'}`}
        className="flex items-center gap-1 px-3 h-9 rounded-full text-sm font-medium text-gray-700 hover:bg-gray-100 transition-colors"
      >
        <Globe size={16} className="text-gray-500" />
        <span>{buttonLabel}</span>
        <ChevronDown size={14} className="text-gray-400" />
      </button>

      {open && (
        <div
          className="absolute bottom-full mb-2 left-1/2 -translate-x-1/2 w-64 rounded-lg bg-white border border-gray-200 shadow-lg overflow-hidden z-50"
          role="dialog"
          aria-label="Pick transcription language"
        >
          {isParakeet && (
            <div className="px-3 py-2 text-xs text-amber-800 bg-amber-50 border-b border-amber-100">
              Parakeet ignores this. Switch the transcription engine to Whisper
              in Settings to use a specific language.
            </div>
          )}

          <div className="flex items-center gap-2 px-3 py-2.5 border-b border-gray-100">
            <span className="text-gray-400 text-sm">🔍</span>
            <input
              ref={inputRef}
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search language..."
              className="flex-1 text-sm text-gray-900 bg-transparent border-none outline-none placeholder-gray-400"
            />
          </div>

          <div className="max-h-72 overflow-y-auto py-1">
            {filtered.map((lang) => {
              const active = lang.code === selectedLanguage;
              const isAuto = lang.code === 'auto' || lang.code === 'auto-translate';
              return (
                <button
                  key={lang.code}
                  type="button"
                  aria-pressed={active}
                  onClick={() => choose(lang.code)}
                  className={`flex w-full items-center justify-between px-3 py-1.5 text-sm hover:bg-gray-50 text-left ${
                    active ? 'text-blue-600 font-medium' : 'text-gray-800'
                  }`}
                >
                  <span>
                    {lang.name}
                    {!isAuto && (
                      <span className="text-xs text-gray-400"> ({lang.code})</span>
                    )}
                  </span>
                  {active && <Check size={14} className="text-blue-600" />}
                </button>
              );
            })}
            {filtered.length === 0 && (
              <div className="px-3 py-2 text-sm text-gray-400">No matches</div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
