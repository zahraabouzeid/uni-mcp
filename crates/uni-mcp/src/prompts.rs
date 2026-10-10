//! Ready-made requests the student picks from a menu in their AI app.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{PromptMessage, Role};
use rmcp::{prompt, prompt_router, schemars};
use serde::Deserialize;

use crate::server::UniServer;

// The doc comments on the argument fields are what the AI app shows next to
// each blank.

#[derive(Deserialize, schemars::JsonSchema)]
pub struct CourseArgs {
    /// The course, by its name or a part of it.
    course: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct LectureArgs {
    /// The course, by its name or a part of it.
    course: String,
    /// A topic or a lecture to explain. Leave empty for the latest lecture.
    topic: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ExerciseArgs {
    /// The course, by its name or a part of it.
    course: String,
    /// Which exercise sheet, e.g. `3`.
    sheet: String,
}

#[prompt_router(vis = "pub(crate)")]
impl UniServer {
    #[prompt(
        name = "my_week",
        title = "My week",
        description = "This week's timetable and the assignments due in it."
    )]
    fn my_week(&self) -> Vec<PromptMessage> {
        ask(
            "Show me my university week. Use list_events for this week, Monday to Sunday, \
             and list it day by day with times and rooms. Then use list_assignments and add \
             the assignments that are due this week. Keep it short.",
        )
    }

    #[prompt(
        name = "deadlines",
        title = "Deadlines",
        description = "All open assignments, the soonest first."
    )]
    fn deadlines(&self) -> Vec<PromptMessage> {
        ask(
            "List my open assignments from list_assignments, sorted by due date with the \
             soonest first. For each one give the course and how many days are left. Leave \
             out assignments whose deadline has passed.",
        )
    }

    #[prompt(
        name = "explain_lecture",
        title = "Explain a lecture",
        description = "Find the slides of a course and explain them."
    )]
    fn explain_lecture(&self, Parameters(args): Parameters<LectureArgs>) -> Vec<PromptMessage> {
        let subject = match args.topic.as_deref().map(str::trim) {
            Some(topic) if !topic.is_empty() => format!("\"{topic}\""),
            _ => "the latest lecture".to_owned(),
        };
        ask(format!(
            "Explain {subject} from my course \"{}\". Find the course with list_courses and \
             its slides with list_course_files, then read the matching file. Look at the \
             pages themselves (view: pages) wherever there are formulas, figures or \
             diagrams. Explain it step by step in plain language, and tell me which file \
             and pages you used.",
            args.course
        ))
    }

    #[prompt(
        name = "exercise_help",
        title = "Help with an exercise",
        description = "Work through an exercise sheet with hints first."
    )]
    fn exercise_help(&self, Parameters(args): Parameters<ExerciseArgs>) -> Vec<PromptMessage> {
        ask(format!(
            "Help me with exercise sheet {} of my course \"{}\". Find the sheet with \
             list_courses and list_course_files and read it as pages (view: pages), so that \
             you see its formulas and figures correctly. Do not give me the full solution \
             straight away: first explain what each task asks for, then guide me with hints, \
             and only show a complete solution when I ask for it.",
            args.sheet, args.course
        ))
    }

    #[prompt(
        name = "summarise_course",
        title = "Summarise a course",
        description = "An overview of a course from all of its material."
    )]
    fn summarise_course(&self, Parameters(args): Parameters<CourseArgs>) -> Vec<PromptMessage> {
        ask(format!(
            "Summarise my course \"{}\" based on all the material you can reach. Go through \
             its files with list_course_files and read them, text first and pages (view: \
             pages) for formulas and figures, and include its assignments. Give me the \
             topics in order, the key ideas of each, and what I should be able to do by the \
             exam. Tell me which files you could not read.",
            args.course
        ))
    }
}

fn ask(request: impl Into<String>) -> Vec<PromptMessage> {
    vec![PromptMessage::new_text(Role::User, request)]
}

#[cfg(test)]
mod tests {
    use rmcp::model::ContentBlock;

    use super::*;
    use crate::connectors::testing::connectors;

    fn text(messages: Vec<PromptMessage>) -> String {
        match &messages[0].content {
            ContentBlock::Text(text) => text.text.clone(),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn offers_the_five_prompts_with_their_blanks() {
        let mut prompts: Vec<(String, Vec<String>)> = UniServer::prompt_router()
            .list_all()
            .into_iter()
            .map(|prompt| {
                let blanks = prompt.arguments.unwrap_or_default();
                (
                    prompt.name,
                    blanks.into_iter().map(|blank| blank.name).collect(),
                )
            })
            .collect();
        prompts.sort();
        let expected = [
            ("deadlines", vec![]),
            ("exercise_help", vec!["course", "sheet"]),
            ("explain_lecture", vec!["course", "topic"]),
            ("my_week", vec![]),
            ("summarise_course", vec!["course"]),
        ];
        assert_eq!(prompts.len(), expected.len());
        for ((name, blanks), (expected_name, expected_blanks)) in prompts.iter().zip(expected) {
            assert_eq!(name, expected_name);
            assert_eq!(blanks, &expected_blanks);
        }
    }

    #[test]
    fn a_prompt_carries_what_the_student_filled_in() {
        let server = UniServer::new(connectors());
        let exercise = text(server.exercise_help(Parameters(ExerciseArgs {
            course: "CES Mechanik I".into(),
            sheet: "3".into(),
        })));
        assert!(exercise.contains("exercise sheet 3"));
        assert!(exercise.contains("\"CES Mechanik I\""));
        assert!(exercise.contains("view: pages"));
    }

    #[test]
    fn a_lecture_without_a_topic_means_the_latest_one() {
        let server = UniServer::new(connectors());
        let lecture = |topic: Option<&str>| {
            text(server.explain_lecture(Parameters(LectureArgs {
                course: "Mechanik".into(),
                topic: topic.map(str::to_owned),
            })))
        };
        assert!(lecture(None).contains("the latest lecture"));
        assert!(lecture(Some("  ")).contains("the latest lecture"));
        assert!(lecture(Some("Kräftegleichgewicht")).contains("\"Kräftegleichgewicht\""));
    }
}
