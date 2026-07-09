// audio/transcription/elevenlabs_provider.rs
//
// ElevenLabs Scribe cloud transcription provider.
//
// Sends each VAD speech chunk to the ElevenLabs speech-to-text API as an
// in-memory WAV upload. Opt-in only: unlike Whisper/Parakeet, audio leaves the
// machine, so this provider is never selected implicitly — the user must pick
// it in Settings → Transcription Models and supply their own API key.

use super::provider::{TranscriptionError, TranscriptionProvider, TranscriptResult};
use async_trait::async_trait;
use log::{error, info};
use serde::Deserialize;
use std::time::Duration;

const API_URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
/// ElevenLabs rejects audio shorter than 100ms; samples are 16kHz mono.
const MIN_SAMPLES: usize = 1600;

#[derive(Debug, Deserialize)]
struct SttResponse {
    #[serde(default)]
    text: String,
}

pub struct ElevenLabsProvider {
    api_key: String,
    model_id: String,
    client: reqwest::Client,
}

impl ElevenLabsProvider {
    pub fn new(api_key: String, model_id: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            api_key,
            model_id,
            client,
        }
    }
}

#[async_trait]
impl TranscriptionProvider for ElevenLabsProvider {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> std::result::Result<TranscriptResult, TranscriptionError> {
        if audio.len() < MIN_SAMPLES {
            return Err(TranscriptionError::AudioTooShort {
                samples: audio.len(),
                minimum: MIN_SAMPLES,
            });
        }

        let wav = pcm16_wav_bytes(&audio, 16000);

        let file_part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| TranscriptionError::EngineFailed(e.to_string()))?;

        let mut form = reqwest::multipart::Form::new()
            .text("model_id", self.model_id.clone())
            // Meeting transcripts don't need "(laughter)"-style annotations.
            .text("tag_audio_events", "false")
            .part("file", file_part);

        // "auto" / "auto-translate" mean no explicit language hint; ElevenLabs
        // auto-detects when language_code is omitted (it never translates).
        if let Some(lang) = language.filter(|l| {
            let l = l.trim();
            !l.is_empty() && l != "auto" && l != "auto-translate"
        }) {
            form = form.text("language_code", lang);
        }

        let response = self
            .client
            .post(API_URL)
            .header("xi-api-key", &self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                error!("ElevenLabs request failed: {}", e);
                TranscriptionError::EngineFailed(format!("ElevenLabs request failed: {}", e))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = match status.as_u16() {
                401 => "Invalid ElevenLabs API key. Check it in Settings → Transcription Models."
                    .to_string(),
                429 => "ElevenLabs rate limit or quota exceeded.".to_string(),
                _ => format!(
                    "ElevenLabs API error {}: {}",
                    status,
                    body.chars().take(300).collect::<String>()
                ),
            };
            error!("{}", message);
            return Err(TranscriptionError::EngineFailed(message));
        }

        let parsed: SttResponse = response
            .json()
            .await
            .map_err(|e| TranscriptionError::EngineFailed(format!("Invalid ElevenLabs response: {}", e)))?;

        info!(
            "ElevenLabs transcribed {} samples -> {} chars",
            audio.len(),
            parsed.text.len()
        );

        Ok(TranscriptResult {
            text: parsed.text.trim().to_string(),
            confidence: None, // API reports language probability, not transcription confidence
            is_partial: false,
        })
    }

    async fn is_model_loaded(&self) -> bool {
        // Cloud provider: "loaded" means we have credentials to call it.
        !self.api_key.trim().is_empty()
    }

    async fn get_current_model(&self) -> Option<String> {
        Some(self.model_id.clone())
    }

    fn provider_name(&self) -> &'static str {
        "ElevenLabs Scribe"
    }
}

/// Encode 16kHz mono f32 samples as a 16-bit PCM WAV byte buffer.
fn pcm16_wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav
}
