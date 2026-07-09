// Speaker channel → human-readable label used in transcript text exports
// (clipboard copies and AI-summary input). 'mic' is the local user, 'system'
// is remote meeting audio, 'mixed' means both channels were active.
// After diarization: 'system:N' = N-th remote speaker, 'speaker:N' = N-th
// speaker in an imported meeting (no channel info).
export function speakerPrefix(speaker?: string): string {
  switch (speaker) {
    case 'mic':
      return 'You: ';
    case 'system':
      return 'Them: ';
    case 'mixed':
      return 'You+Them: ';
  }
  const match = speaker?.match(/^(system|speaker):(\d+)$/);
  if (match) {
    return match[1] === 'system' ? `Them ${match[2]}: ` : `Speaker ${match[2]}: `;
  }
  return '';
}
