//! Custom LangChain transcription tool: langchain-rust currently only ships TTS.
use super::{MAX_AUDIO_BYTES, SpeechError, SpeechRecognizer};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use langchain_rust::tools::Tool;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{error::Error, time::Duration};

pub struct OpenAiSpeechRecognizer {
    tool: OpenAiTranscriptionTool,
}

impl OpenAiSpeechRecognizer {
    pub fn new(key: String) -> Result<Self, reqwest::Error> {
        Ok(Self {
            tool: OpenAiTranscriptionTool {
                client: reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(90))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()?,
                key,
                endpoint: "https://api.openai.com/v1/audio/transcriptions".into(),
            },
        })
    }
}

#[async_trait]
impl SpeechRecognizer for OpenAiSpeechRecognizer {
    async fn transcribe(&self, audio: Vec<u8>) -> Result<String, SpeechError> {
        if audio.is_empty() || audio.len() > MAX_AUDIO_BYTES {
            return Err(SpeechError::InvalidAudio);
        }
        // Use run directly: Tool::call's default parser logs its input.
        // Provider/SDK errors never cross this boundary or enter logs.
        self.tool
            .run(Value::String(STANDARD.encode(audio)))
            .await
            .map_err(|_| SpeechError::RecognitionFailed)
    }
}

struct OpenAiTranscriptionTool {
    client: reqwest::Client,
    key: String,
    endpoint: String,
}

#[derive(Deserialize)]
struct Transcript {
    text: String,
}

#[async_trait]
impl Tool for OpenAiTranscriptionTool {
    fn name(&self) -> String {
        "transcribe_voice".into()
    }

    fn description(&self) -> String {
        "Transcribe a base64-encoded Ogg/Opus voice recording in its original language.".into()
    }

    // Override the default parser to avoid logging private audio.
    async fn parse_input(&self, input: &str) -> Value {
        Value::String(input.into())
    }

    async fn run(&self, input: Value) -> Result<String, Box<dyn Error>> {
        let invalid = || SpeechError::InvalidAudio;
        let encoded = input.as_str().ok_or_else(invalid)?;
        if encoded.len() > MAX_AUDIO_BYTES.div_ceil(3) * 4 {
            return Err(invalid().into());
        }
        let audio = STANDARD.decode(encoded).map_err(|_| invalid())?;
        if audio.is_empty() || audio.len() > MAX_AUDIO_BYTES {
            return Err(invalid().into());
        }
        let form = reqwest::multipart::Form::new()
            .text("model", "gpt-4o-mini-transcribe")
            .text("response_format", "json")
            .part(
                "file",
                reqwest::multipart::Part::bytes(audio)
                    .file_name("voice.ogg")
                    .mime_str("audio/ogg")?,
            );
        let transcript: Transcript = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.key)
            .multipart(form)
            .send()
            .await
            .map_err(|_| SpeechError::RecognitionFailed)?
            .error_for_status()
            .map_err(|_| SpeechError::RecognitionFailed)?
            .json()
            .await
            .map_err(|_| SpeechError::RecognitionFailed)?;
        let text = transcript.text.trim();
        if text.is_empty() {
            return Err(SpeechError::RecognitionFailed.into());
        }
        Ok(text.into())
    }

    fn parameters(&self) -> Value {
        json!({"type":"string", "description":self.description()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn langchain_tool_uploads_ogg_with_explicit_key_and_handles_provider_responses() {
        for (status, body, expected) in [
            (
                StatusCode::OK,
                r#"{"text":"  Hello <world> 😀  "}"#,
                Some("Hello <world> 😀"),
            ),
            (StatusCode::OK, r#"{"text":" "}"#, None),
            (StatusCode::OK, r#"{"invalid":true}"#, None),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"error":"secret-test-key"}"#,
                None,
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                r#"{"error":"rate limit"}"#,
                None,
            ),
        ] {
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = requests.clone();
            let app = Router::new().route(
                "/audio/transcriptions",
                post(move |headers: HeaderMap, bytes: Bytes| {
                    captured.lock().unwrap().push((headers, bytes));
                    async move { (status, [("content-type", "application/json")], body) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let mut recognizer = OpenAiSpeechRecognizer::new("secret-test-key".into()).unwrap();
            recognizer.tool.endpoint = format!("http://{address}/audio/transcriptions");
            let result = recognizer.transcribe(b"OggS test audio".to_vec()).await;
            match expected {
                Some(text) => assert_eq!(result.unwrap(), text),
                None => {
                    let error = result.unwrap_err();
                    assert!(matches!(error, SpeechError::RecognitionFailed));
                    assert!(!format!("{error:?}: {error}").contains("secret-test-key"));
                }
            }
            assert!(matches!(
                recognizer.transcribe(Vec::new()).await,
                Err(SpeechError::InvalidAudio)
            ));
            assert!(matches!(
                recognizer.transcribe(vec![0; MAX_AUDIO_BYTES + 1]).await,
                Err(SpeechError::InvalidAudio)
            ));
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].0["authorization"], "Bearer secret-test-key");
            let multipart = String::from_utf8_lossy(&requests[0].1);
            for part in [
                "name=\"model\"",
                "gpt-4o-mini-transcribe",
                "name=\"file\"; filename=\"voice.ogg\"",
                "audio/ogg",
                "OggS test audio",
            ] {
                assert!(multipart.contains(part), "missing {part}");
            }
            server.abort();
        }
    }
}
