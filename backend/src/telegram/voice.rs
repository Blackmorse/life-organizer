use super::TelegramSender;
use crate::speech::{MAX_AUDIO_BYTES, SpeechError};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct FileResponse {
    ok: bool,
    result: Option<TelegramFile>,
}

#[derive(Deserialize)]
struct TelegramFile {
    file_path: Option<String>,
    file_size: Option<u64>,
}

impl TelegramSender {
    pub(super) async fn download_audio(&self, file_id: &str) -> Result<Vec<u8>, SpeechError> {
        let failed = || SpeechError::RecognitionFailed;
        let file: FileResponse = self
            .call("getFile", json!({"file_id": file_id}))
            .await
            .map_err(|_| failed())?
            .json()
            .await
            .map_err(|_| failed())?;
        if !file.ok {
            return Err(failed());
        }
        let file = file.result.ok_or_else(failed)?;
        if file
            .file_size
            .is_some_and(|size| size > MAX_AUDIO_BYTES as u64)
        {
            return Err(SpeechError::InvalidAudio);
        }
        let path = file.file_path.ok_or_else(failed)?;
        // File paths must stay beneath Telegram's file endpoint.
        if path.is_empty()
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !path
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/_-.".contains(&c))
        {
            return Err(failed());
        }
        let (base, suffix) = self.endpoint.rsplit_once("/bot").ok_or_else(failed)?;
        let token = suffix.strip_suffix("/sendMessage").ok_or_else(failed)?;
        let mut response = self
            .client
            .get(format!("{base}/file/bot{token}/{path}"))
            .send()
            .await
            .map_err(|_| failed())?
            .error_for_status()
            .map_err(|_| failed())?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_AUDIO_BYTES as u64)
        {
            return Err(SpeechError::InvalidAudio);
        }
        let mut audio = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| failed())? {
            if chunk.len() > MAX_AUDIO_BYTES - audio.len() {
                return Err(SpeechError::InvalidAudio);
            }
            audio.extend_from_slice(&chunk);
        }
        if audio.is_empty() {
            return Err(SpeechError::InvalidAudio);
        }
        Ok(audio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morning::MorningBot;
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn resolves_and_downloads_voice_with_metadata_and_stream_size_checks() {
        for (metadata, bytes, succeeds, downloads) in [
            (
                json!({"ok":true,"result":{"file_path":"voice/file.oga","file_size":4}}),
                b"OggS".to_vec(),
                true,
                1,
            ),
            (json!({"ok":false}), vec![], false, 0),
            (json!({"ok":true,"result":{}}), vec![], false, 0),
            (
                json!({"ok":true,"result":{"file_path":"../secret"}}),
                vec![],
                false,
                0,
            ),
            (
                json!({"ok":true,"result":{"file_path":"https://evil.example/file"}}),
                vec![],
                false,
                0,
            ),
            (
                json!({"ok":true,"result":{"file_path":"voice/file.oga","file_size":MAX_AUDIO_BYTES+1}}),
                vec![],
                false,
                0,
            ),
            (
                json!({"ok":true,"result":{"file_path":"voice/file.oga"}}),
                vec![0; MAX_AUDIO_BYTES + 1],
                false,
                1,
            ),
            (
                json!({"ok":true,"result":{"file_path":"voice/file.oga"}}),
                vec![],
                false,
                1,
            ),
        ] {
            let downloaded = Arc::new(AtomicUsize::new(0));
            let captured = downloaded.clone();
            let app = Router::new()
                .route(
                    "/botsecret/getFile",
                    post(move |Json(body): Json<serde_json::Value>| async move {
                        assert_eq!(body, json!({"file_id":"voice-id"}));
                        Json(metadata)
                    }),
                )
                .route(
                    "/file/botsecret/voice/file.oga",
                    get(move || {
                        captured.fetch_add(1, Ordering::SeqCst);
                        async move { bytes }
                    }),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let mut sender = TelegramSender::new("secret").unwrap();
            sender.endpoint = format!("http://{address}/botsecret/sendMessage");
            let result = sender.download_voice("voice-id").await;
            if succeeds {
                assert_eq!(result.unwrap(), b"OggS");
            } else {
                assert!(!format!("{:?}", result.unwrap_err()).contains("secret"));
            }
            assert_eq!(downloaded.load(Ordering::SeqCst), downloads);
            server.abort();
        }
    }
}
