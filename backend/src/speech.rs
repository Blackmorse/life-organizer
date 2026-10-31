//! Provider-neutral audio transcription boundary. Audio stays in memory.
pub mod openai;

use async_trait::async_trait;

// Telegram's hosted Bot API downloads at most 20 MB.
pub const MAX_AUDIO_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum SpeechError {
    #[error("Voice message is empty or exceeds the 20 MB limit. Please send a shorter recording.")]
    InvalidAudio,
    #[error("Could not recognize speech. Please try recording your message again.")]
    RecognitionFailed,
}

#[async_trait]
pub trait SpeechRecognizer: Send + Sync {
    /// Recognize an in-memory Ogg/Opus recording in its original language.
    async fn transcribe(&self, audio: Vec<u8>) -> Result<String, SpeechError>;
}
