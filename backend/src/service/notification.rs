use crate::domain::DailyTaskSummary;
use async_trait::async_trait;

/// Delivery boundary for independently valid Telegram HTML messages.
/// Application logic does not depend on an HTTP client.
#[async_trait]
pub trait MessageSender: Send + Sync {
    async fn send(&self, recipient: i64, messages: &[String]) -> Result<(), NotificationError>;
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum NotificationError {
    #[error(
        "Open the bot's private chat, press Start (or send /start), and unblock the bot before finalizing tasks."
    )]
    ChatUnavailable,
    #[error(
        "Could not confirm Telegram delivery. Check the bot chat before retrying; some messages may already have arrived."
    )]
    DeliveryFailed,
}

pub(super) fn daily_messages(items: &[DailyTaskSummary]) -> Vec<String> {
    summary_messages(items, "Tasks for today are ready!", "")
}

pub(crate) fn summary_messages(
    items: &[DailyTaskSummary],
    title: &str,
    footer: &str,
) -> Vec<String> {
    let mut groups = std::collections::BTreeMap::<i64, Vec<&DailyTaskSummary>>::new();
    for item in items {
        groups.entry(item.task_id).or_default().push(item);
    }
    let mut html = HtmlMessages::default();
    html.append(title, true);
    html.append("\n\n", false);
    for group in groups.values() {
        // Context-only parents are not selected work, so have no status emoji.
        let parent = group.iter().find(|item| item.subtask.is_none());
        let prefix = parent.map_or("", |item| if item.completed { "✅ " } else { "🔹 " });
        html.append(&format!("{prefix}{}", group[0].task), true);
        html.append("\n", false);
        for item in group.iter().filter(|item| item.subtask.is_some()) {
            let mark = if item.completed { "✅" } else { "🔹" };
            // NBSP indentation survives Telegram rendering and explicit newlines.
            let description = item
                .subtask
                .as_ref()
                .unwrap()
                .replace('\n', "\n\u{a0}\u{a0}\u{a0}\u{a0}  ");
            html.append(
                &format!("\u{a0}\u{a0}\u{a0}\u{a0}{mark} {description}\n"),
                false,
            );
        }
        html.append("\n", false);
    }
    html.append(footer, false);
    html.flush();
    html.messages
}

/// Split rendered text, never raw HTML. Each message has balanced tags and whole
/// escaped entities; the 4096-unit limit applies after parsing HTML entities.
#[derive(Default)]
struct HtmlMessages {
    messages: Vec<String>,
    current: String,
    units: usize,
}

impl HtmlMessages {
    fn flush(&mut self) {
        if !self.current.trim().is_empty() {
            self.messages.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
        self.units = 0;
    }

    fn append(&mut self, text: &str, bold: bool) {
        let length = text.encode_utf16().count();
        if length <= 4096 && self.units + length > 4096 {
            self.flush();
        }
        let mut segment = String::new();
        for character in text.chars() {
            if self.units + character.len_utf16() > 4096 {
                self.append_segment(&segment, bold);
                segment.clear();
                self.flush();
            }
            match character {
                '&' => segment.push_str("&amp;"),
                '<' => segment.push_str("&lt;"),
                '>' => segment.push_str("&gt;"),
                _ => segment.push(character),
            }
            self.units += character.len_utf16();
        }
        self.append_segment(&segment, bold);
    }

    fn append_segment(&mut self, segment: &str, bold: bool) {
        if segment.is_empty() {
            return;
        }
        if bold {
            self.current.push_str("<b>");
        }
        self.current.push_str(segment);
        if bold {
            self.current.push_str("</b>");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_unicode_lists_are_complete_and_within_telegram_limits() {
        let item = DailyTaskSummary {
            task_id: 1,
            category: "Work <&>".into(),
            task: "<&>😀".repeat(2000),
            subtask: Some("<&>😀".repeat(2000)),
            completed: true,
        };
        let messages = daily_messages(std::slice::from_ref(&item));
        assert!(messages.len() > 1);
        let visible = |part: &str| {
            part.replace("<b>", "")
                .replace("</b>", "")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
        };
        for part in &messages {
            assert!(visible(part).encode_utf16().count() <= 4096);
            assert_eq!(part.matches("<b>").count(), part.matches("</b>").count());
            assert!(!part.replace("<b>", "").replace("</b>", "").contains('<'));
            assert!(
                !part
                    .replace("&lt;", "")
                    .replace("&amp;", "")
                    .replace("&gt;", "")
                    .contains('&')
            );
        }
        assert_eq!(
            visible(&messages.concat()),
            format!(
                "Tasks for today are ready!\n\n{}\n\u{a0}\u{a0}\u{a0}\u{a0}✅ {}\n\n",
                item.task,
                item.subtask.unwrap(),
            )
        );
    }

    #[test]
    fn groups_selected_steps_by_identity_and_uses_status_emoji() {
        let item = |task_id, task: &str, subtask: Option<&str>, completed| DailyTaskSummary {
            task_id,
            category: "Hidden category".into(),
            task: task.into(),
            subtask: subtask.map(String::from),
            completed,
        };
        let messages = daily_messages(&[
            item(1, "Build <app>", Some("Design & plan"), false),
            item(1, "Build <app>", Some("Ship"), true),
            item(2, "Clean", None, false),
            item(3, "Build <app>", None, true),
            item(3, "Build <app>", Some("Follow up"), false),
        ]);
        assert_eq!(
            messages,
            vec![
                "<b>Tasks for today are ready!</b>\n\n<b>Build &lt;app&gt;</b>\n\u{a0}\u{a0}\u{a0}\u{a0}🔹 Design &amp; plan\n\u{a0}\u{a0}\u{a0}\u{a0}✅ Ship\n\n<b>🔹 Clean</b>\n\n<b>✅ Build &lt;app&gt;</b>\n\u{a0}\u{a0}\u{a0}\u{a0}🔹 Follow up\n\n"
            ]
        );
    }
}
