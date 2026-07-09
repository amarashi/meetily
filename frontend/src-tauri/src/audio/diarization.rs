// Speaker diarization module - distinguishes individual speakers in a saved meeting.
//
// Runs fully locally with sherpa-onnx (pyannote segmentation + 3D-Speaker
// embeddings, ~46MB of ONNX models downloaded once into the models dir).
//
// This refines the coarse channel-based labels assigned live by the audio
// pipeline: "mic" segments (the local user, tagged from the microphone channel)
// are kept as ground truth, while "system" segments (all remote participants
// mixed on one channel) are split into distinct speakers and relabeled
// "system:1", "system:2", ... For meetings with no channel info at all
// (imported files), segments get generic "speaker:1", "speaker:2", ... labels.

use crate::audio::decoder::decode_audio_file;
use crate::audio::retranscription::find_audio_file;
use crate::state::AppState;
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Only one diarization job at a time (each loads ~50MB of models and is CPU heavy)
static DIARIZATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Pyannote segmentation 3.0 exported for sherpa-onnx (~6MB)
const SEGMENTATION_MODEL_FILE: &str = "sherpa-onnx-pyannote-segmentation-3-0.onnx";
const SEGMENTATION_MODEL_URL: &str =
    "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx";

/// 3D-Speaker ERes2Net speaker embedding model (~38MB). Trained on Mandarin but
/// speaker embeddings transfer across languages (used here for English/Persian).
const EMBEDDING_MODEL_FILE: &str = "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";
const EMBEDDING_MODEL_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";

/// Agglomerative clustering threshold when the speaker count is unknown.
/// Lower = more speakers detected; 0.5 is the sherpa-onnx recommended default.
const CLUSTERING_THRESHOLD: f32 = 0.5;

/// A diarized speaker turn on the recording timeline (seconds)
#[derive(Debug, Clone, Copy)]
struct SpeakerTurn {
    start: f64,
    end: f64,
    speaker: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationProgress {
    pub meeting_id: String,
    pub stage: String, // "models", "decoding", "diarizing", "saving"
    pub progress_percentage: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationResult {
    pub meeting_id: String,
    pub num_speakers: usize,
    pub segments_updated: usize,
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    stage: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress {
            meeting_id: meeting_id.to_string(),
            stage: stage.to_string(),
            progress_percentage: progress,
            message: message.to_string(),
        },
    );
}

/// Download a model file to the models dir (atomic: temp file + rename)
async fn download_model(url: &str, dest: &Path) -> Result<()> {
    info!("Downloading diarization model from {}", url);

    let response = reqwest::get(url).await?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Model download failed with HTTP {}: {}",
            response.status(),
            url
        ));
    }

    let temp_path = dest.with_extension("download");
    let mut file = tokio::fs::File::create(&temp_path).await?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    drop(file);

    tokio::fs::rename(&temp_path, dest).await?;
    info!("Downloaded diarization model to {}", dest.display());
    Ok(())
}

/// Ensure both diarization models exist locally, downloading them if missing.
/// Returns (segmentation_model_path, embedding_model_path).
async fn ensure_models<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
) -> Result<(PathBuf, PathBuf)> {
    let models_dir = crate::models_base_dir(app).join("diarization");
    tokio::fs::create_dir_all(&models_dir).await?;

    let seg_path = models_dir.join(SEGMENTATION_MODEL_FILE);
    let emb_path = models_dir.join(EMBEDDING_MODEL_FILE);

    if !seg_path.exists() {
        emit_progress(app, meeting_id, "models", 5, "Downloading segmentation model (6MB)...");
        download_model(SEGMENTATION_MODEL_URL, &seg_path).await?;
    }
    if !emb_path.exists() {
        emit_progress(app, meeting_id, "models", 10, "Downloading speaker embedding model (38MB)...");
        download_model(EMBEDDING_MODEL_URL, &emb_path).await?;
    }

    Ok((seg_path, emb_path))
}

/// Run sherpa-onnx offline diarization over 16kHz mono samples
fn run_diarization(
    seg_model: &Path,
    emb_model: &Path,
    samples: &[f32],
) -> Result<Vec<SpeakerTurn>> {
    let mut config = sherpa_onnx::OfflineSpeakerDiarizationConfig::default();
    config.segmentation.pyannote.model = Some(seg_model.to_string_lossy().into_owned());
    config.segmentation.num_threads = 2;
    config.embedding.model = Some(emb_model.to_string_lossy().into_owned());
    config.embedding.num_threads = 2;
    // -1 clusters = automatic speaker count via threshold
    config.clustering.num_clusters = -1;
    config.clustering.threshold = CLUSTERING_THRESHOLD;
    // Ignore speaker turns shorter than 300ms and merge gaps under 500ms
    config.min_duration_on = 0.3;
    config.min_duration_off = 0.5;

    let diarizer = sherpa_onnx::OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| anyhow!("Failed to create diarizer (check model files)"))?;

    let expected_rate = diarizer.sample_rate();
    if expected_rate != 16000 {
        warn!(
            "Diarizer expects {}Hz but audio is 16kHz - results may be degraded",
            expected_rate
        );
    }

    let result = diarizer
        .process(samples)
        .ok_or_else(|| anyhow!("Diarization processing failed"))?;

    let turns = result
        .sort_by_start_time()
        .into_iter()
        .map(|s| SpeakerTurn {
            start: s.start as f64,
            end: s.end as f64,
            speaker: s.speaker,
        })
        .collect::<Vec<_>>();

    info!(
        "Diarization found {} speakers in {} turns",
        result.num_speakers(),
        turns.len()
    );
    Ok(turns)
}

/// Overlap in seconds between two time ranges
fn overlap(a_start: f64, a_end: f64, b_start: f64, b_end: f64) -> f64 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

/// One transcript row as loaded from the DB
type TranscriptRow = (String, Option<f64>, Option<f64>, Option<String>);

/// Decide the new speaker labels for transcript rows given the diarized turns.
///
/// Strategy:
/// - The diarized cluster that overlaps mostly with "mic"-labeled rows is the
///   local user; live channel attribution stays authoritative for those rows.
/// - Remaining clusters become "Them 1", "Them 2", ... (stored "system:N"),
///   numbered by order of first appearance in the meeting.
/// - Meetings without any channel info (imports) get "speaker:N" labels.
///
/// Returns (row_id, new_label) pairs for rows whose label should change,
/// plus the number of distinct non-you speakers found.
fn relabel_rows(rows: &[TranscriptRow], turns: &[SpeakerTurn]) -> (Vec<(String, String)>, usize) {
    if turns.is_empty() {
        return (Vec::new(), 0);
    }

    let has_mic_rows = rows
        .iter()
        .any(|(_, _, _, speaker)| speaker.as_deref() == Some("mic"));

    // Per-cluster: total speaking time and time overlapping mic-labeled rows
    let max_cluster = turns.iter().map(|t| t.speaker).max().unwrap_or(0) as usize;
    let mut cluster_total = vec![0.0f64; max_cluster + 1];
    let mut cluster_mic_overlap = vec![0.0f64; max_cluster + 1];

    for turn in turns {
        let c = turn.speaker as usize;
        cluster_total[c] += turn.end - turn.start;
        for (_, start, end, speaker) in rows {
            if speaker.as_deref() == Some("mic") {
                if let (Some(s), Some(e)) = (start, end) {
                    cluster_mic_overlap[c] += overlap(turn.start, turn.end, *s, *e);
                }
            }
        }
    }

    // The cluster spending most of its time inside mic-labeled rows is "You"
    let you_cluster: Option<usize> = if has_mic_rows {
        cluster_total
            .iter()
            .enumerate()
            .filter(|(_, total)| **total > 0.0)
            .map(|(c, total)| (c, cluster_mic_overlap[c] / total))
            .filter(|(_, ratio)| *ratio > 0.5)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(c, _)| c)
    } else {
        None
    };

    // Number remaining clusters 1..N by order of first appearance
    let mut speaker_numbers = vec![None::<usize>; max_cluster + 1];
    let mut next_number = 1usize;
    for turn in turns {
        let c = turn.speaker as usize;
        if Some(c) != you_cluster && speaker_numbers[c].is_none() {
            speaker_numbers[c] = Some(next_number);
            next_number += 1;
        }
    }
    let num_speakers = next_number - 1;

    // Multiple distinct remote speakers are needed for relabeling to add
    // information; with 0-1 the channel-based labels are already as good.
    if num_speakers < 2 {
        return (Vec::new(), num_speakers);
    }

    let label_prefix = if has_mic_rows { "system" } else { "speaker" };

    let mut updates = Vec::new();
    for (id, start, end, speaker) in rows {
        // Live channel attribution stays authoritative for the local user and
        // overlapping speech; only remote/unknown rows are refined.
        match speaker.as_deref() {
            Some("mic") | Some("mixed") => continue,
            _ => {}
        }
        let (Some(s), Some(e)) = (start, end) else {
            continue;
        };

        // Dominant diarized cluster over this row's time window
        let mut per_cluster = vec![0.0f64; max_cluster + 1];
        for turn in turns {
            per_cluster[turn.speaker as usize] += overlap(turn.start, turn.end, *s, *e);
        }
        let dominant = per_cluster
            .iter()
            .enumerate()
            .filter(|(_, ov)| **ov > 0.0)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(c, _)| c);

        let Some(dominant) = dominant else {
            continue; // No diarized speech overlaps this row
        };

        if Some(dominant) == you_cluster {
            continue; // Bleed/echo of the local user - keep the channel label
        }

        if let Some(number) = speaker_numbers[dominant] {
            let new_label = format!("{}:{}", label_prefix, number);
            if speaker.as_deref() != Some(new_label.as_str()) {
                updates.push((id.clone(), new_label));
            }
        }
    }

    (updates, num_speakers)
}

/// Run diarization for a saved meeting and refine its transcript speaker labels
async fn run_meeting_diarization<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
) -> Result<DiarizationResult> {
    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;
    let pool = app_state.db_manager.pool();

    // 1. Meeting folder and transcript rows
    let folder_path: Option<(Option<String>,)> =
        sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_optional(pool)
            .await?;
    let folder = folder_path
        .and_then(|(f,)| f)
        .ok_or_else(|| anyhow!("Meeting {} has no folder path (no audio to diarize)", meeting_id))?;

    let rows: Vec<TranscriptRow> = sqlx::query_as(
        "SELECT id, audio_start_time, audio_end_time, speaker FROM transcripts WHERE meeting_id = ?",
    )
    .bind(&meeting_id)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Err(anyhow!("Meeting {} has no transcripts", meeting_id));
    }

    // 2. Models
    let (seg_model, emb_model) = ensure_models(&app, &meeting_id).await?;

    // 3. Decode meeting audio to 16kHz mono
    emit_progress(&app, &meeting_id, "decoding", 20, "Decoding meeting audio...");
    let audio_path = find_audio_file(Path::new(&folder))?;
    let samples = tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
        let decoded = decode_audio_file(&audio_path)?;
        Ok(decoded.to_whisper_format())
    })
    .await
    .map_err(|e| anyhow!("Decode task panicked: {}", e))??;

    info!(
        "Diarizing meeting {}: {:.1}s of audio, {} transcript segments",
        meeting_id,
        samples.len() as f64 / 16000.0,
        rows.len()
    );

    // 4. Diarize (CPU heavy - run on blocking thread)
    emit_progress(&app, &meeting_id, "diarizing", 40, "Detecting speakers...");
    let turns = tokio::task::spawn_blocking(move || {
        run_diarization(&seg_model, &emb_model, &samples)
    })
    .await
    .map_err(|e| anyhow!("Diarization task panicked: {}", e))??;

    // 5. Relabel transcript rows
    emit_progress(&app, &meeting_id, "saving", 85, "Assigning speakers to transcript...");
    let (updates, num_speakers) = relabel_rows(&rows, &turns);

    if !updates.is_empty() {
        let mut conn = pool.acquire().await?;
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        for (row_id, label) in &updates {
            sqlx::query("UPDATE transcripts SET speaker = ? WHERE id = ?")
                .bind(label)
                .bind(row_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
    }

    info!(
        "Diarization for meeting {}: {} remote speakers, {} segments relabeled",
        meeting_id,
        num_speakers,
        updates.len()
    );

    Ok(DiarizationResult {
        meeting_id,
        num_speakers,
        segments_updated: updates.len(),
    })
}

/// Tauri command: diarize a saved meeting's audio and refine speaker labels.
/// Emits "diarization-complete" or "diarization-error" when done.
#[tauri::command]
pub async fn diarize_meeting<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
) -> Result<DiarizationResult, String> {
    if DIARIZATION_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Speaker detection already in progress".to_string());
    }

    let result = run_meeting_diarization(app.clone(), meeting_id.clone()).await;
    DIARIZATION_IN_PROGRESS.store(false, Ordering::SeqCst);

    match result {
        Ok(res) => {
            let _ = app.emit("diarization-complete", &res);
            Ok(res)
        }
        Err(e) => {
            warn!("Diarization failed for meeting {}: {}", meeting_id, e);
            let _ = app.emit(
                "diarization-error",
                serde_json::json!({ "meeting_id": meeting_id, "error": e.to_string() }),
            );
            Err(e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, start: f64, end: f64, speaker: Option<&str>) -> TranscriptRow {
        (id.to_string(), Some(start), Some(end), speaker.map(String::from))
    }

    #[test]
    fn test_relabel_splits_remote_speakers() {
        // You (mic) at 0-10, remote speaker A at 10-20, remote speaker B at 20-30
        let rows = vec![
            row("r1", 0.0, 10.0, Some("mic")),
            row("r2", 10.0, 20.0, Some("system")),
            row("r3", 20.0, 30.0, Some("system")),
        ];
        // Diarizer: cluster 0 = you, cluster 1 = A, cluster 2 = B
        let turns = vec![
            SpeakerTurn { start: 0.0, end: 10.0, speaker: 0 },
            SpeakerTurn { start: 10.0, end: 20.0, speaker: 1 },
            SpeakerTurn { start: 20.0, end: 30.0, speaker: 2 },
        ];

        let (updates, num_speakers) = relabel_rows(&rows, &turns);
        assert_eq!(num_speakers, 2);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0], ("r2".to_string(), "system:1".to_string()));
        assert_eq!(updates[1], ("r3".to_string(), "system:2".to_string()));
    }

    #[test]
    fn test_relabel_single_remote_speaker_keeps_labels() {
        let rows = vec![
            row("r1", 0.0, 10.0, Some("mic")),
            row("r2", 10.0, 20.0, Some("system")),
        ];
        let turns = vec![
            SpeakerTurn { start: 0.0, end: 10.0, speaker: 0 },
            SpeakerTurn { start: 10.0, end: 20.0, speaker: 1 },
        ];

        // Only one remote speaker: "Them" is already the right label
        let (updates, num_speakers) = relabel_rows(&rows, &turns);
        assert_eq!(num_speakers, 1);
        assert!(updates.is_empty());
    }

    #[test]
    fn test_relabel_import_without_channel_info() {
        // Imported meeting: no mic rows, NULL speakers
        let rows = vec![
            row("r1", 0.0, 10.0, None),
            row("r2", 10.0, 20.0, None),
        ];
        let turns = vec![
            SpeakerTurn { start: 0.0, end: 10.0, speaker: 0 },
            SpeakerTurn { start: 10.0, end: 20.0, speaker: 1 },
        ];

        let (updates, num_speakers) = relabel_rows(&rows, &turns);
        assert_eq!(num_speakers, 2);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].1, "speaker:1");
        assert_eq!(updates[1].1, "speaker:2");
    }

    #[test]
    fn test_relabel_keeps_mic_and_mixed_rows() {
        let rows = vec![
            row("r1", 0.0, 10.0, Some("mic")),
            row("r2", 10.0, 20.0, Some("mixed")),
            row("r3", 20.0, 30.0, Some("system")),
            row("r4", 30.0, 40.0, Some("system")),
        ];
        let turns = vec![
            SpeakerTurn { start: 0.0, end: 10.0, speaker: 0 },
            SpeakerTurn { start: 10.0, end: 20.0, speaker: 1 },
            SpeakerTurn { start: 20.0, end: 30.0, speaker: 1 },
            SpeakerTurn { start: 30.0, end: 40.0, speaker: 2 },
        ];

        let (updates, _) = relabel_rows(&rows, &turns);
        // mic and mixed rows are never touched
        assert!(updates.iter().all(|(id, _)| id != "r1" && id != "r2"));
        assert_eq!(updates.len(), 2);
    }
}
