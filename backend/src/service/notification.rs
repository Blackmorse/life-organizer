use crate::domain::{DailyTaskSource, DailyTaskSummary};
use async_trait::async_trait;

pub(super) fn backlog_messages(items: &[crate::domain::BacklogTask]) -> Vec<String> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut html = HtmlMessages::default();
    html.append("Still on your plan — outside Tasks of the day", true);
    html.append("\n\n", false);
    let mut previous = None;
    let mut previous_category = None;
    for item in items {
        if previous != Some(item.urgency) {
            if previous.is_some() {
                html.append("\n", false);
            }
            let heading = match item.urgency {
                crate::domain::TaskUrgency::Soon => "🔥 Soon — within days",
                crate::domain::TaskUrgency::Upcoming => "📅 Upcoming — within weeks",
                crate::domain::TaskUrgency::Someday => "🌱 Someday",
            };
            html.append(heading, true);
            html.append("\n", false);
            previous = Some(item.urgency);
            previous_category = None;
        }
        if previous_category != Some(item.category_id) {
            if previous_category.is_some() {
                html.append("\n", false);
            }
            html.append(&indented(&format!("📁 {}", item.category), 1), true);
            html.append("\n", false);
            previous_category = Some(item.category_id);
        }
        html.append(&indented(&format!("🔹 {}", item.description), 2), true);
        html.append("\n", false);
    }
    html.flush();
    html.messages
}

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
    if items.is_empty() {
        return Vec::new();
    }
    // Summary rows are selected work, never the context-only parent headings.
    let completed = items.iter().filter(|item| item.completed).count();
    let total = items.len();
    let filled = completed * 10 / total;
    let footer = format!(
        "Progress today: {completed}/{total} tasks completed\n{}{} {}%",
        "■".repeat(filled),
        "□".repeat(10 - filled),
        completed * 100 / total,
    );
    summary_messages(items, "Tasks for today are ready!", &footer)
}

pub(crate) fn summary_messages(
    items: &[DailyTaskSummary],
    title: &str,
    footer: &str,
) -> Vec<String> {
    let mut html = HtmlMessages::default();
    append_summary(&mut html, items, title, footer);
    html.flush();
    html.messages
}

/// Keep notices and the actionable offer together, splitting only at Telegram's
/// size limit. The final question always accompanies the Yes/No buttons.
pub(crate) fn morning_messages(
    items: &[DailyTaskSummary],
    tomorrow: &[DailyTaskSummary],
    yesterday: &[DailyTaskSummary],
) -> Vec<String> {
    let mut html = HtmlMessages::default();
    for (section, title, footer) in [
        (
            tomorrow,
            "Note: tomorrow there are scheduled one-time tasks",
            "These reminders are for tomorrow; they are not added to today's list.",
        ),
        (
            items,
            "Suggested tasks for today: planned tasks, scheduled work, reminders and unfinished daily picks",
            "",
        ),
        (
            yesterday,
            "Note: you haven't completed these reminder tasks from yesterday",
            "These reminders are not added to today's list.",
        ),
    ] {
        if !section.is_empty() {
            if !html.current.is_empty() {
                html.append("\n\n", false);
            }
            append_summary(&mut html, section, title, footer);
        }
    }
    if !items.is_empty() {
        html.append("\n\nAdd these tasks from Suggested tasks for today to Tasks of the day? Use /morning to show this offer again.", false);
    }
    html.flush();
    html.messages
}

fn append_summary(html: &mut HtmlMessages, items: &[DailyTaskSummary], title: &str, footer: &str) {
    let mut categories = std::collections::BTreeMap::<
        (DailyTaskSource, Option<i64>),
        std::collections::BTreeMap<i64, Vec<&DailyTaskSummary>>,
    >::new();
    for item in items {
        categories
            .entry((item.source, item.category_id))
            .or_default()
            .entry(item.task_id)
            .or_default()
            .push(item);
    }
    html.append(title, true);
    html.append("\n\n", false);
    for ((source, _), groups) in categories {
        let heading = match source {
            DailyTaskSource::Ordinary => {
                format!("📁 {}", groups.values().next().unwrap()[0].category)
            }
            DailyTaskSource::Scheduled => "🔁 Scheduled".into(),
            DailyTaskSource::Reminder => "⏰ Reminders".into(),
        };
        html.append(&indented(&heading, 0), true);
        html.append("\n", false);
        for group in groups.values() {
            // Context-only parents get a task icon rather than a completion mark.
            let parent = group.iter().find(|item| item.subtask.is_none());
            let prefix = parent.map_or("📋 ", |item| if item.completed { "✅ " } else { "🔹 " });
            html.append(&indented(&format!("{prefix}{}", group[0].task), 1), true);
            html.append("\n", false);
            for item in group.iter().filter(|item| item.subtask.is_some()) {
                let mark = if item.completed { "✅" } else { "🔹" };
                html.append(
                    &indented(&format!("{mark} {}", item.subtask.as_ref().unwrap()), 2),
                    false,
                );
                html.append("\n", false);
            }
            html.append("\n", false);
        }
    }
    html.append(footer, false);
}

// NBSP indentation survives Telegram rendering; continuation lines stay nested.
fn indented(text: &str, depth: usize) -> String {
    let padding = "\u{a0}".repeat(depth * 4);
    format!("{padding}{}", text.replace('\n', &format!("\n{padding}  ")))
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
    fn morning_sections_share_one_message_with_unambiguous_final_choice() {
        let item = |task: &str| DailyTaskSummary {
            source: DailyTaskSource::Reminder,
            task_id: 1,
            category_id: None,
            category: String::new(),
            task: task.into(),
            subtask: None,
            completed: false,
        };
        let messages = morning_messages(
            &[item("Today <&>")],
            &[item("Tomorrow")],
            &[item("Yesterday")],
        );
        assert_eq!(messages.len(), 1);
        let text = &messages[0];
        assert!(text.contains("Today &lt;&amp;&gt;"));
        assert!(text.find("Yesterday").unwrap() < text.find("Add these tasks").unwrap());
        assert!(text.ends_with("Use /morning to show this offer again."));
        assert!(!text.contains("Progress today:"));

        let messages = morning_messages(&[item(&"😀<&>".repeat(3000))], &[], &[item("Yesterday")]);
        assert!(messages.len() > 1);
        for text in &messages {
            let rendered = text
                .replace("<b>", "")
                .replace("</b>", "")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&");
            assert!(rendered.encode_utf16().count() <= 4096);
            assert_eq!(text.matches("<b>").count(), text.matches("</b>").count());
        }
        assert!(
            messages
                .last()
                .unwrap()
                .contains("Add these tasks from Suggested tasks for today")
        );
        assert!(morning_messages(&[], &[], &[]).is_empty());
        assert!(
            !morning_messages(&[], &[], &[item("Yesterday")])
                .concat()
                .contains("Add these tasks")
        );
    }

    #[test]
    fn backlog_nests_categories_under_urgency_and_tasks_under_categories() {
        use crate::domain::{BacklogTask, TaskUrgency};
        let item = |category_id, category: &str, description: &str, urgency| BacklogTask {
            category_id,
            category: category.into(),
            description: description.into(),
            urgency,
        };
        let messages = backlog_messages(&[
            item(1, "Work <&>", "First", TaskUrgency::Soon),
            item(1, "Work <&>", "Second\nline", TaskUrgency::Soon),
            item(2, "Work <&>", "Separate category", TaskUrgency::Soon),
            item(1, "Work <&>", "Later", TaskUrgency::Upcoming),
            item(1, "Work <&>", "Eventually", TaskUrgency::Someday),
        ]);
        let indent = "\u{a0}".repeat(4);
        assert_eq!(
            messages,
            vec![format!(
                "<b>Still on your plan — outside Tasks of the day</b>\n\n\
             <b>🔥 Soon — within days</b>\n\
             <b>{indent}📁 Work &lt;&amp;&gt;</b>\n\
             <b>{indent}{indent}🔹 First</b>\n\
             <b>{indent}{indent}🔹 Second\n{indent}{indent}  line</b>\n\n\
             <b>{indent}📁 Work &lt;&amp;&gt;</b>\n\
             <b>{indent}{indent}🔹 Separate category</b>\n\n\
             <b>📅 Upcoming — within weeks</b>\n\
             <b>{indent}📁 Work &lt;&amp;&gt;</b>\n\
             <b>{indent}{indent}🔹 Later</b>\n\n\
             <b>🌱 Someday</b>\n\
             <b>{indent}📁 Work &lt;&amp;&gt;</b>\n\
             <b>{indent}{indent}🔹 Eventually</b>\n"
            )]
        );
    }

    #[test]
    fn long_unicode_lists_are_complete_and_within_telegram_limits() {
        let item = DailyTaskSummary {
            source: DailyTaskSource::Ordinary,
            task_id: 1,
            category_id: Some(1),
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
                "Tasks for today are ready!\n\n📁 Work <&>\n{}📋 {}\n{}✅ {}\n\nProgress today: 1/1 tasks completed\n■■■■■■■■■■ 100%",
                "\u{a0}".repeat(4),
                item.task,
                "\u{a0}".repeat(8),
                item.subtask.unwrap(),
            )
        );
    }

    #[test]
    fn groups_selected_steps_by_identity_and_uses_status_emoji() {
        let item = |task_id, task: &str, subtask: Option<&str>, completed| DailyTaskSummary {
            source: DailyTaskSource::Ordinary,
            task_id,
            category_id: Some(1),
            category: "Work <&>".into(),
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
            vec![format!(
                "<b>Tasks for today are ready!</b>\n\n<b>📁 Work &lt;&amp;&gt;</b>\n<b>{indent}📋 Build &lt;app&gt;</b>\n{indent}{indent}🔹 Design &amp; plan\n{indent}{indent}✅ Ship\n\n<b>{indent}🔹 Clean</b>\n\n<b>{indent}✅ Build &lt;app&gt;</b>\n{indent}{indent}🔹 Follow up\n\nProgress today: 2/5 tasks completed\n■■■■□□□□□□ 40%",
                indent = "\u{a0}".repeat(4)
            )]
        );
    }

    #[test]
    fn daily_progress_counts_all_sources_and_handles_empty_zero_and_full_progress() {
        assert!(daily_messages(&[]).is_empty());
        let mut items = vec![
            DailyTaskSummary {
                source: DailyTaskSource::Ordinary,
                task_id: 1,
                category_id: Some(1),
                category: "Work".into(),
                task: "Build".into(),
                subtask: Some("Ship".into()),
                completed: false,
            },
            DailyTaskSummary {
                source: DailyTaskSource::Scheduled,
                task_id: -1,
                category_id: None,
                category: String::new(),
                task: "Exercise".into(),
                subtask: Some("Warm up".into()),
                completed: false,
            },
            DailyTaskSummary {
                source: DailyTaskSource::Reminder,
                task_id: 1,
                category_id: None,
                category: String::new(),
                task: "Appointment".into(),
                subtask: None,
                completed: false,
            },
        ];
        for (completed, expected) in [
            (0, "Progress today: 0/3 tasks completed\n□□□□□□□□□□ 0%"),
            (1, "Progress today: 1/3 tasks completed\n■■■□□□□□□□ 33%"),
            (3, "Progress today: 3/3 tasks completed\n■■■■■■■■■■ 100%"),
        ] {
            for (index, item) in items.iter_mut().enumerate() {
                item.completed = index < completed;
            }
            let messages = daily_messages(&items);
            assert!(messages.last().unwrap().ends_with(expected));
            assert_eq!(messages.concat().matches("Progress today:").count(), 1);
        }
    }

    #[test]
    fn category_identity_scheduled_section_and_multiline_indentation_are_preserved() {
        let item = |task_id, category_id: Option<i64>, task: &str, subtask: Option<&str>| {
            DailyTaskSummary {
                source: if category_id.is_some() {
                    DailyTaskSource::Ordinary
                } else {
                    DailyTaskSource::Scheduled
                },
                task_id,
                category_id,
                category: "Same <&>\ncategory".into(),
                task: task.into(),
                subtask: subtask.map(String::from),
                completed: false,
            }
        };
        let messages = summary_messages(
            &[
                item(1, Some(1), "First\ntask", Some("First\nstep")),
                item(2, Some(2), "Second", None),
                item(3, Some(1), "Third", None),
                item(-1, None, "Exercise", Some("Warm up")),
            ],
            "Morning",
            "Add these?",
        );
        let text = messages.concat();
        let indent = "\u{a0}".repeat(4);
        assert_eq!(text.matches("📁 Same &lt;&amp;&gt;").count(), 2);
        assert_eq!(text.matches("🔁 Scheduled").count(), 1);
        assert!(text.find("Third").unwrap() < text.find("Second").unwrap());
        assert!(text.contains(&format!("<b>{indent}📋 First\n{indent}  task</b>")));
        assert!(text.contains(&format!("{indent}{indent}🔹 First\n{indent}{indent}  step")));
        assert!(text.contains(&format!(
            "<b>{indent}📋 Exercise</b>\n{indent}{indent}🔹 Warm up"
        )));
        assert!(text.ends_with("Add these?"));
        assert!(!text.contains("Progress today:"));
    }
}
