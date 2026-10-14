use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::DecisionError;

const CHOICE_OPTIONS: std::ops::RangeInclusive<usize> = 2..=255;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "UncheckedRequest")]
pub struct DecisionRequest {
    input: DecisionInput,
    questions: Vec<Question>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UncheckedRequest {
    input: DecisionInput,
    questions: Vec<Question>,
}

impl TryFrom<UncheckedRequest> for DecisionRequest {
    type Error = DecisionError;

    fn try_from(request: UncheckedRequest) -> Result<Self, Self::Error> {
        Self::new(request.input, request.questions)
    }
}

impl DecisionRequest {
    pub fn new(
        input: impl Into<DecisionInput>,
        questions: impl IntoIterator<Item = Question>,
    ) -> Result<Self, DecisionError> {
        let input = input.into();
        if matches!(&input, DecisionInput::Parts(parts) if parts.is_empty()) {
            return Err(invalid("multimodal input needs at least one part"));
        }
        let questions: Vec<Question> = questions.into_iter().collect();
        if questions.is_empty() {
            return Err(invalid("at least one question is required"));
        }
        let mut names = HashSet::with_capacity(questions.len());
        for question in &questions {
            if question.name.is_empty() {
                return Err(invalid("every question needs a non-empty name"));
            }
            if !names.insert(question.name.as_str()) {
                return Err(invalid(format!(
                    "question name `{}` is used more than once",
                    question.name
                )));
            }
            question.kind.validate(&question.name)?;
        }
        Ok(Self { input, questions })
    }

    #[must_use]
    pub const fn input(&self) -> &DecisionInput {
        &self.input
    }

    #[must_use]
    pub fn questions(&self) -> &[Question] {
        &self.questions
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DecisionInput {
    Text(String),
    Parts(Vec<InputPart>),
    Record(Map<String, Value>),
}

impl From<String> for DecisionInput {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for DecisionInput {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<Vec<InputPart>> for DecisionInput {
    fn from(parts: Vec<InputPart>) -> Self {
        Self::Parts(parts)
    }
}

impl From<Map<String, Value>> for DecisionInput {
    fn from(record: Map<String, Value>) -> Self {
        Self::Record(record)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum InputPart {
    #[serde(rename = "input_text")]
    Text { text: String },
    #[serde(rename = "input_image")]
    Image {
        image_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
}

impl InputPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image(image_url: impl Into<String>) -> Self {
        Self::Image {
            image_url: image_url.into(),
            detail: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    Auto,
    Low,
    High,
    Original,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "TaggedQuestion")]
pub struct Question {
    pub name: String,
    pub instructions: String,
    #[serde(flatten)]
    pub kind: QuestionKind,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum TaggedQuestion {
    Predicate {
        name: String,
        instructions: String,
    },
    Choice {
        name: String,
        instructions: String,
        choices: Vec<ChoiceOption>,
    },
    Score {
        name: String,
        instructions: String,
        levels: Vec<ScoreLevel>,
    },
}

impl From<TaggedQuestion> for Question {
    fn from(question: TaggedQuestion) -> Self {
        match question {
            TaggedQuestion::Predicate { name, instructions } => Self::predicate(name, instructions),
            TaggedQuestion::Choice {
                name,
                instructions,
                choices,
            } => Self::choice(name, instructions, choices),
            TaggedQuestion::Score {
                name,
                instructions,
                levels,
            } => Self::score(name, instructions, levels),
        }
    }
}

impl Question {
    pub fn predicate(name: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            instructions: instructions.into(),
            kind: QuestionKind::Predicate,
        }
    }

    pub fn choice(
        name: impl Into<String>,
        instructions: impl Into<String>,
        choices: impl IntoIterator<Item = ChoiceOption>,
    ) -> Self {
        Self {
            name: name.into(),
            instructions: instructions.into(),
            kind: QuestionKind::Choice {
                choices: choices.into_iter().collect(),
            },
        }
    }

    pub fn score(
        name: impl Into<String>,
        instructions: impl Into<String>,
        levels: impl IntoIterator<Item = ScoreLevel>,
    ) -> Self {
        Self {
            name: name.into(),
            instructions: instructions.into(),
            kind: QuestionKind::Score {
                levels: levels.into_iter().collect(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestionKind {
    Predicate,
    Choice { choices: Vec<ChoiceOption> },
    Score { levels: Vec<ScoreLevel> },
}

impl QuestionKind {
    fn validate(&self, name: &str) -> Result<(), DecisionError> {
        match self {
            Self::Predicate => Ok(()),
            Self::Choice { choices } => {
                if !CHOICE_OPTIONS.contains(&choices.len()) {
                    return Err(invalid(format!(
                        "choice question `{name}` needs between {} and {} options",
                        CHOICE_OPTIONS.start(),
                        CHOICE_OPTIONS.end()
                    )));
                }
                let mut values = HashSet::with_capacity(choices.len());
                for choice in choices {
                    if !values.insert(&choice.value) {
                        return Err(invalid(format!(
                            "choice question `{name}` repeats the option {}",
                            choice.value
                        )));
                    }
                }
                Ok(())
            }
            Self::Score { levels } if levels.is_empty() => Err(invalid(format!(
                "score question `{name}` needs at least one level"
            ))),
            Self::Score { .. } => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceOption {
    pub value: ChoiceValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ChoiceOption {
    pub fn new(value: impl Into<ChoiceValue>) -> Self {
        Self {
            value: value.into(),
            description: None,
        }
    }

    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChoiceValue {
    Bool(bool),
    Text(String),
}

impl std::fmt::Display for ChoiceValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Text(value) => write!(f, "{value:?}"),
        }
    }
}

impl From<bool> for ChoiceValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for ChoiceValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ChoiceValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreLevel {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ScoreLevel {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: None,
        }
    }

    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

fn invalid(message: impl Into<String>) -> DecisionError {
    DecisionError::InvalidRequest(message.into())
}
