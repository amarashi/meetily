'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Mic, Wand2, FileText } from 'lucide-react';
import { useConfig } from '@/contexts/ConfigContext';

interface DictationSettings {
  cleanup_enabled: boolean;
  cleanup_model: string;
  ollama_endpoint: string;
}

// Human-readable label for a transcription provider string.
function transcriptionLabel(provider: string | undefined, model: string | undefined): string {
  switch (provider) {
    case 'parakeet':
      return `Parakeet · ${model ?? '?'}`;
    case 'localWhisper':
      return `Whisper (local) · ${model ?? '?'}`;
    case 'elevenLabs':
      return `ElevenLabs (cloud) · ${model ?? 'scribe_v2'}`;
    default:
      return model ? `${provider} · ${model}` : (provider ?? 'not set');
  }
}

/**
 * Slim always-visible line showing which model handles each stage:
 * transcription (speech→text), text cleanup (dictation LLM pass), and
 * meeting summaries. Answers "where is my audio/text going?" at a glance —
 * especially relevant now that transcription can be a cloud provider.
 */
export function ModelStatusStrip() {
  const { transcriptModelConfig, modelConfig } = useConfig();
  const [dictation, setDictation] = useState<DictationSettings | null>(null);

  useEffect(() => {
    invoke<DictationSettings>('get_dictation_settings')
      .then(setDictation)
      .catch(() => setDictation(null));
  }, []);

  const cleanupLabel = dictation
    ? dictation.cleanup_enabled
      ? `${dictation.cleanup_model} (Ollama)`
      : 'off'
    : '…';

  const summaryLabel = modelConfig?.model
    ? `${modelConfig.provider} · ${modelConfig.model}`
    : 'not set';

  const item = 'flex items-center gap-1 whitespace-nowrap';

  return (
    <div className="flex items-center justify-center gap-4 text-[11px] text-gray-400 select-none">
      <span className={item} title="Transcription engine (speech → text)">
        <Mic size={11} />
        {transcriptionLabel(transcriptModelConfig?.provider, transcriptModelConfig?.model)}
      </span>
      <span className={item} title="Dictation text cleanup model">
        <Wand2 size={11} />
        {cleanupLabel}
      </span>
      <span className={item} title="Meeting summary model">
        <FileText size={11} />
        {summaryLabel}
      </span>
    </div>
  );
}
