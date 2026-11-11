//! Voice transcription and shared text/voice daily-task assistance.
use crate::{
    bot_commands::BotMessage,
    morning::MorningBot,
    service::NotificationError,
    speech::{MAX_AUDIO_BYTES, SpeechError, SpeechRecognizer},
    task_assistant::{AssistantReply, VoiceAssistant},
};

#[derive(serde::Deserialize)]
pub struct VoiceMessage {
    pub file_id: String,
    pub file_size: Option<u64>,
}

pub async fn handle(
    message: &BotMessage,
    bot: &dyn MorningBot,
    recognizer: &dyn SpeechRecognizer,
    assistant: Option<&dyn VoiceAssistant>,
) -> Result<(), NotificationError> {
    if message.sender <= 0 || message.private_chat != Some(message.sender) {
        return Ok(());
    }
    let Some(voice) = &message.voice else {
        return Ok(());
    };
    let result = async {
        if voice
            .file_size
            .is_some_and(|size| size > MAX_AUDIO_BYTES as u64)
        {
            return Err(SpeechError::InvalidAudio);
        }
        let audio = bot.download_voice(&voice.file_id).await?;
        recognizer.transcribe(audio).await
    }
    .await;
    let reply = match result {
        Ok(text) if !text.trim().is_empty() => respond(message.sender, &text, assistant).await,
        Ok(_) => plain_reply(SpeechError::RecognitionFailed.to_string()),
        Err(error) => plain_reply(error.to_string()),
    };
    send_reply(message.sender, bot, reply).await
}

/// Typed requests enter at the same boundary as recognized speech. Slash
/// commands (including unknown/addressed forms) belong to command dispatch.
pub async fn handle_text(
    message: &BotMessage,
    bot: &dyn MorningBot,
    assistant: &dyn VoiceAssistant,
) -> Result<(), NotificationError> {
    if message.sender <= 0
        || message.private_chat != Some(message.sender)
        || message.voice.is_some()
        || message.text.trim().is_empty()
        || message.text.trim_start().starts_with('/')
    {
        return Ok(());
    }
    let reply = respond(message.sender, &message.text, Some(assistant)).await;
    send_reply(message.sender, bot, reply).await
}

async fn respond(
    sender: i64,
    text: &str,
    assistant: Option<&dyn VoiceAssistant>,
) -> AssistantReply {
    match assistant {
        Some(assistant) => assistant
            .respond(sender, text)
            .await
            .unwrap_or_else(|error| plain_reply(error.to_string())),
        None => plain_reply(text.into()),
    }
}

async fn send_reply(
    sender: i64,
    bot: &dyn MorningBot,
    reply: AssistantReply,
) -> Result<(), NotificationError> {
    let mut messages = text_messages(&reply.text);
    messages.extend(reply.daily_messages);
    bot.send(sender, &messages).await
}

fn plain_reply(text: String) -> AssistantReply {
    AssistantReply {
        text,
        daily_messages: Vec::new(),
    }
}

// MessageSender accepts HTML. Escape each scalar after splitting by rendered
// UTF-16 length so entities and surrogate pairs cannot straddle messages.
fn text_messages(text: &str) -> Vec<String> {
    let mut messages = Vec::new();
    let mut chunk = String::new();
    let mut units = 0;
    for c in text.chars() {
        if units + c.len_utf16() > 4096 {
            messages.push(std::mem::take(&mut chunk));
            units = 0;
        }
        match c {
            '&' => chunk.push_str("&amp;"),
            '<' => chunk.push_str("&lt;"),
            '>' => chunk.push_str("&gt;"),
            c => chunk.push(c),
        }
        units += c.len_utf16();
    }
    if !chunk.is_empty() {
        messages.push(chunk);
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{morning::BotUpdate, service::MessageSender};
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeBot {
        downloads: Mutex<Vec<String>>,
        sent: Mutex<Vec<(i64, Vec<String>)>>,
        fail_download: bool,
        fail_send: bool,
    }

    #[async_trait]
    impl MessageSender for FakeBot {
        async fn send(&self, recipient: i64, messages: &[String]) -> Result<(), NotificationError> {
            self.sent
                .lock()
                .unwrap()
                .push((recipient, messages.to_vec()));
            if self.fail_send {
                Err(NotificationError::DeliveryFailed)
            } else {
                Ok(())
            }
        }
    }

    #[async_trait]
    impl MorningBot for FakeBot {
        async fn download_voice(&self, file_id: &str) -> Result<Vec<u8>, SpeechError> {
            self.downloads.lock().unwrap().push(file_id.into());
            if self.fail_download {
                Err(SpeechError::RecognitionFailed)
            } else {
                Ok(b"OggS".to_vec())
            }
        }
        async fn configure_commands(&self) -> Result<(), NotificationError> {
            unreachable!()
        }
        async fn offer(&self, _: i64, _: i64, _: &[String]) -> Result<(), NotificationError> {
            unreachable!()
        }
        async fn updates(&self, _: i64) -> Result<Vec<BotUpdate>, NotificationError> {
            unreachable!()
        }
        async fn answer(&self, _: &str, _: &str) -> Result<(), NotificationError> {
            unreachable!()
        }
    }

    struct Recognizer {
        result: Result<String, SpeechError>,
        calls: Mutex<usize>,
    }
    #[async_trait]
    impl SpeechRecognizer for Recognizer {
        async fn transcribe(&self, audio: Vec<u8>) -> Result<String, SpeechError> {
            assert_eq!(audio, b"OggS");
            *self.calls.lock().unwrap() += 1;
            self.result.clone()
        }
    }

    fn message() -> BotMessage {
        BotMessage {
            sender: 123,
            private_chat: Some(123),
            text: String::new(),
            voice: Some(VoiceMessage {
                file_id: "recording".into(),
                file_size: None,
            }),
        }
    }

    #[tokio::test]
    async fn returns_escaped_transcript_only_to_matching_private_sender() {
        let bot = FakeBot::default();
        let recognizer = Recognizer {
            result: Ok("Привет <b>& 😀".into()),
            calls: Mutex::new(0),
        };
        for (sender, chat) in [(123, None), (123, Some(456)), (0, Some(0)), (-1, Some(-1))] {
            let mut message = message();
            message.sender = sender;
            message.private_chat = chat;
            handle(&message, &bot, &recognizer, None).await.unwrap();
        }
        assert!(bot.downloads.lock().unwrap().is_empty());
        handle(&message(), &bot, &recognizer, None).await.unwrap();
        assert_eq!(*bot.downloads.lock().unwrap(), ["recording"]);
        assert_eq!(*recognizer.calls.lock().unwrap(), 1);
        assert_eq!(
            *bot.sent.lock().unwrap(),
            [(123, vec!["Привет &lt;b&gt;&amp; 😀".into()])]
        );
    }

    #[tokio::test]
    async fn rejects_oversize_before_download_and_reports_download_provider_and_empty_results() {
        for case in 0..4 {
            let bot = FakeBot {
                fail_download: case == 1,
                ..Default::default()
            };
            let recognizer = Recognizer {
                result: if case == 2 {
                    Err(SpeechError::RecognitionFailed)
                } else {
                    Ok(" ".into())
                },
                calls: Mutex::new(0),
            };
            let mut message = message();
            if case == 0 {
                message.voice.as_mut().unwrap().file_size = Some(MAX_AUDIO_BYTES as u64 + 1);
            }
            handle(&message, &bot, &recognizer, None).await.unwrap();
            assert_eq!(*recognizer.calls.lock().unwrap(), usize::from(case >= 2));
            let sent = bot.sent.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert!(sent[0].1[0].contains(if case == 0 {
                "20 MB"
            } else {
                "Could not recognize speech"
            }));
        }
    }

    #[tokio::test]
    async fn failed_reply_is_not_retried() {
        let bot = FakeBot {
            fail_send: true,
            ..Default::default()
        };
        let recognizer = Recognizer {
            result: Ok("Hello".into()),
            calls: Mutex::new(0),
        };
        assert!(handle(&message(), &bot, &recognizer, None).await.is_err());
        assert_eq!(*recognizer.calls.lock().unwrap(), 1);
        assert_eq!(bot.sent.lock().unwrap().len(), 1);
    }

    struct Assistant {
        calls: Mutex<Vec<(i64, String)>>,
        fail: bool,
    }
    #[async_trait]
    impl VoiceAssistant for Assistant {
        async fn respond(
            &self,
            sender: i64,
            text: &str,
        ) -> Result<AssistantReply, crate::task_assistant::AssistantError> {
            self.calls.lock().unwrap().push((sender, text.into()));
            if self.fail {
                Err(crate::task_assistant::AssistantError)
            } else {
                Ok(AssistantReply {
                    text: "Completed: <buy groceries>".into(),
                    daily_messages: vec!["<b>📁 Home</b>\n✅ &lt;buy groceries&gt;".into()],
                })
            }
        }
    }

    #[tokio::test]
    async fn passes_transcript_and_verified_sender_to_assistant_and_escapes_result() {
        for (fail, fail_send) in [(false, false), (true, false), (false, true)] {
            let bot = FakeBot {
                fail_send,
                ..Default::default()
            };
            let recognizer = Recognizer {
                result: Ok("Mark groceries completed".into()),
                calls: Mutex::new(0),
            };
            let assistant = Assistant {
                calls: Mutex::new(Vec::new()),
                fail,
            };
            let mut foreign = message();
            foreign.private_chat = Some(456);
            handle(&foreign, &bot, &recognizer, Some(&assistant))
                .await
                .unwrap();
            assert!(assistant.calls.lock().unwrap().is_empty());
            assert_eq!(
                handle(&message(), &bot, &recognizer, Some(&assistant))
                    .await
                    .is_err(),
                fail_send
            );
            assert_eq!(
                *assistant.calls.lock().unwrap(),
                [(123, "Mark groceries completed".into())]
            );
            let sent = bot.sent.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].1.len(), if fail { 1 } else { 2 });
            if !fail {
                assert_eq!(sent[0].1[1], "<b>📁 Home</b>\n✅ &lt;buy groceries&gt;");
            }
            assert_eq!(
                sent[0].1[0],
                if fail {
                    crate::task_assistant::AssistantError.to_string()
                } else {
                    "Completed: &lt;buy groceries&gt;".into()
                }
            );
        }
    }

    #[test]
    fn splits_long_transcripts_without_breaking_entities_or_unicode() {
        let text = format!("{}😀<&>", "x".repeat(4095));
        let chunks = text_messages(&text);
        assert_eq!(chunks, ["x".repeat(4095), "😀&lt;&amp;&gt;".into()]);
    }

    #[tokio::test]
    async fn typed_requests_share_assistant_replies_and_failure_handling() {
        for (fail, fail_send) in [(false, false), (true, false), (false, true)] {
            let bot = FakeBot {
                fail_send,
                ..Default::default()
            };
            let assistant = Assistant {
                calls: Mutex::new(Vec::new()),
                fail,
            };
            let mut message = message();
            message.voice = None;
            message.text = "Please mark <buy groceries> completed".into();
            assert_eq!(
                handle_text(&message, &bot, &assistant).await.is_err(),
                fail_send
            );
            assert_eq!(
                *assistant.calls.lock().unwrap(),
                [(123, message.text.clone())]
            );
            assert!(bot.downloads.lock().unwrap().is_empty());
            let sent = bot.sent.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].0, 123);
            if fail {
                assert_eq!(
                    sent[0].1,
                    [crate::task_assistant::AssistantError.to_string()]
                );
            } else {
                assert_eq!(
                    sent[0].1,
                    [
                        "Completed: &lt;buy groceries&gt;",
                        "<b>📁 Home</b>\n✅ &lt;buy groceries&gt;",
                    ]
                );
            }
        }
    }

    #[tokio::test]
    async fn typed_requests_ignore_commands_blank_voice_and_unverified_chats() {
        let bot = FakeBot::default();
        let assistant = Assistant {
            calls: Mutex::new(Vec::new()),
            fail: false,
        };
        for text in ["", " \n", "/today", " /help", "/unknown", "/today@bot"] {
            let mut message = message();
            message.voice = None;
            message.text = text.into();
            handle_text(&message, &bot, &assistant).await.unwrap();
        }
        for (sender, chat) in [(123, None), (123, Some(456)), (0, Some(0)), (-1, Some(-1))] {
            let mut message = message();
            message.voice = None;
            message.text = "Complete groceries".into();
            message.sender = sender;
            message.private_chat = chat;
            handle_text(&message, &bot, &assistant).await.unwrap();
        }
        handle_text(&message(), &bot, &assistant).await.unwrap();
        assert!(assistant.calls.lock().unwrap().is_empty());
        assert!(bot.sent.lock().unwrap().is_empty());
    }
}
