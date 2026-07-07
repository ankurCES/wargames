//! Optional ElevenLabs TTS client. Fails soft when the voice section is
//! missing — the TUI prints "(tts disabled)" instead of speaking.
//!
//! The whole module is feature-staged: API key + voice are loaded from
//! settings but the network call is gated behind a future ElevenLabs
//! setup step. Suppressing dead_code here is a deliberate hold-over.
#![allow(dead_code)]

use crate::config::BlumiSettings;
use crate::net::with_ceiling;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Tts {
    pub api_key: Option<String>,
    pub voice: Option<String>,
    pub enabled: bool,
}

impl Tts {
    pub fn from_settings(settings: &BlumiSettings) -> Self {
        let v = settings.voice.clone();
        Self {
            enabled: v.as_ref().map(|x| x.enabled).unwrap_or(false),
            api_key: v.as_ref().and_then(|x| x.tts_api_key.clone()),
            voice: v.as_ref().and_then(|x| x.tts_voice.clone()),
        }
    }

    pub fn disabled_reason(&self) -> Option<&'static str> {
        if !self.enabled {
            Some("(tts disabled)")
        } else if self.api_key.is_none() {
            Some("(tts: no api key)")
        } else {
            None
        }
    }

    pub async fn speak(&self, _text: &str) -> Result<(), TtsError> {
        if !self.enabled {
            return Err(TtsError::Disabled);
        }
        let api_key = self
            .api_key
            .as_ref()
            .ok_or(TtsError::Disabled)?;
        let voice = self.voice.clone().unwrap_or_else(|| "alloy".to_string());
        let body = ElevenLabsRequest {
            text: _text,
            model_id: "eleven_multilingual_v2",
            voice_settings: VoiceSettings {
                stability: 0.5,
                similarity_boost: 0.75,
            },
        };
        let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{}", voice);
        let client = reqwest::Client::new();
        let req = client
            .post(&url)
            .header("xi-api-key", api_key)
            .header("content-type", "application/json")
            .json(&body);
        // We don't actually play the audio in TTY — we just verify the
        // request reaches ElevenLabs. A real player integration is
        // out of scope for this rewrite.
        let resp = with_ceiling(req.send())
            .await
            .map_err(|_| TtsError::Timeout)?
            .map_err(TtsError::Reqwest)?;
        if !resp.status().is_success() {
            return Err(TtsError::Upstream(resp.status().as_u16()));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct ElevenLabsRequest<'a> {
    text: &'a str,
    model_id: &'a str,
    voice_settings: VoiceSettings,
}

#[derive(Debug, Serialize)]
struct VoiceSettings {
    stability: f32,
    similarity_boost: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("(tts disabled)")]
    Disabled,
    #[error("tts request timed out (12s ceiling)")]
    Timeout,
    #[error("tts upstream returned {0}")]
    Upstream(u16),
    #[error("tts http error: {0}")]
    Reqwest(reqwest::Error),
}