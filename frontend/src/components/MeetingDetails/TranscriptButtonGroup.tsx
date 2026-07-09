"use client";

import { useState, useCallback, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, FolderOpen, RefreshCw, Undo2, Users } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import Analytics from '@/lib/analytics';
import { RetranscribeDialog } from './RetranscribeDialog';
import { useConfig } from '@/contexts/ConfigContext';


interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
}


export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onOpenMeetingFolder,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
}: TranscriptButtonGroupProps) {
  const { betaFeatures } = useConfig();
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);
  const [isDetectingSpeakers, setIsDetectingSpeakers] = useState(false);
  const [backupCount, setBackupCount] = useState(0);
  const [isRestoring, setIsRestoring] = useState(false);

  const refreshBackupCount = useCallback(async () => {
    if (!meetingFolderPath) {
      setBackupCount(0);
      return;
    }
    try {
      const count = await invoke<number>('list_transcript_backups_command', {
        meetingFolderPath,
      });
      setBackupCount(count);
    } catch {
      setBackupCount(0);
    }
  }, [meetingFolderPath]);

  useEffect(() => {
    refreshBackupCount();
  }, [refreshBackupCount]);

  const handleRetranscribeComplete = useCallback(async () => {
    // Refetch transcripts to show the updated data
    if (onRefetchTranscripts) {
      await onRefetchTranscripts();
    }
    await refreshBackupCount();
  }, [onRefetchTranscripts, refreshBackupCount]);

  // Undo the last Enhance: restore the pre-retranscription transcript backup
  const handleUndoEnhance = useCallback(async () => {
    if (!meetingId || !meetingFolderPath || isRestoring) return;
    setIsRestoring(true);
    try {
      const restored = await invoke<number>('restore_transcript_backup_command', {
        meetingId,
        meetingFolderPath,
      });
      toast.success('Previous transcript restored', {
        description: `${restored} segments recovered.`,
      });
      if (onRefetchTranscripts) {
        await onRefetchTranscripts();
      }
    } catch (error) {
      toast.error('Failed to restore transcript', { description: String(error) });
    } finally {
      setIsRestoring(false);
      await refreshBackupCount();
    }
  }, [meetingId, meetingFolderPath, isRestoring, onRefetchTranscripts, refreshBackupCount]);

  // Run local speaker diarization over the recording and tag segments
  // with Them 1/2/... (or Speaker 1/2/... for imported meetings)
  const handleDetectSpeakers = useCallback(async () => {
    if (!meetingId || isDetectingSpeakers) return;
    setIsDetectingSpeakers(true);
    try {
      const result = await invoke<{ num_speakers: number; segments_updated: number }>(
        'diarize_meeting',
        { meetingId }
      );
      if (result.segments_updated > 0) {
        toast.success(`Detected ${result.num_speakers} speakers`, {
          description: `${result.segments_updated} transcript segments tagged.`,
        });
        if (onRefetchTranscripts) {
          await onRefetchTranscripts();
        }
      } else {
        toast.info('No additional speakers detected', {
          description: 'The existing speaker labels are already as detailed as possible.',
        });
      }
    } catch (error) {
      toast.error('Speaker detection failed', { description: String(error) });
    } finally {
      setIsDetectingSpeakers(false);
    }
  }, [meetingId, isDetectingSpeakers, onRefetchTranscripts]);

  return (
    <div className="flex items-center justify-center w-full gap-2">
      <ButtonGroup>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            Analytics.trackButtonClick('copy_transcript', 'meeting_details');
            onCopyTranscript();
          }}
          disabled={transcriptCount === 0}
          title={transcriptCount === 0 ? 'No transcript available' : 'Copy Transcript'}
        >
          <Copy />
          <span className="hidden lg:inline">Copy</span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          className="xl:px-4"
          onClick={() => {
            Analytics.trackButtonClick('open_recording_folder', 'meeting_details');
            onOpenMeetingFolder();
          }}
          title="Open Recording Folder"
        >
          <FolderOpen className="xl:mr-2" size={18} />
          <span className="hidden lg:inline">Recording</span>
        </Button>

        {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
          <Button
            size="sm"
            variant="outline"
            className="bg-gradient-to-r from-blue-50 to-purple-50 hover:from-blue-100 hover:to-purple-100 border-blue-200 xl:px-4"
            onClick={() => {
              Analytics.trackButtonClick('enhance_transcript', 'meeting_details');
              setShowRetranscribeDialog(true);
            }}
            title="Retranscribe to enhance your recorded audio"
          >
            <RefreshCw className="xl:mr-2" size={18} />
            <span className="hidden lg:inline">Enhance</span>
          </Button>
        )}

        {meetingId && meetingFolderPath && backupCount > 0 && (
          <Button
            size="sm"
            variant="outline"
            className="xl:px-4"
            onClick={() => {
              Analytics.trackButtonClick('undo_enhance_transcript', 'meeting_details');
              handleUndoEnhance();
            }}
            disabled={isRestoring}
            title="Restore the transcript as it was before the last Enhance"
          >
            <Undo2 className={`xl:mr-2 ${isRestoring ? 'animate-pulse' : ''}`} size={18} />
            <span className="hidden lg:inline">{isRestoring ? 'Restoring...' : 'Undo'}</span>
          </Button>
        )}

        {meetingId && meetingFolderPath && transcriptCount > 0 && (
          <Button
            size="sm"
            variant="outline"
            className="xl:px-4"
            onClick={() => {
              Analytics.trackButtonClick('detect_speakers', 'meeting_details');
              handleDetectSpeakers();
            }}
            disabled={isDetectingSpeakers}
            title="Detect individual speakers in the recording (local AI)"
          >
            <Users className={`xl:mr-2 ${isDetectingSpeakers ? 'animate-pulse' : ''}`} size={18} />
            <span className="hidden lg:inline">{isDetectingSpeakers ? 'Detecting...' : 'Speakers'}</span>
          </Button>
        )}
      </ButtonGroup>

      {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
        <RetranscribeDialog
          open={showRetranscribeDialog}
          onOpenChange={setShowRetranscribeDialog}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onComplete={handleRetranscribeComplete}
        />
      )}
    </div>
  );
}
