use serde::Serialize;

use crate::{ChoiceValue, DecisionError, DecisionRequest, QuestionKind};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Decision {
    model: String,
    answers: Vec<Answer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<DecisionUsage>,
}

impl Decision {
    pub fn new(
        request: &DecisionRequest,
        model: impl Into<String>,
        answers: Vec<Answer>,
        usage: Option<DecisionUsage>,
    ) -> Result<Self, DecisionError> {
        let questions = request.questions();
        if answers.len() != questions.len() {
            return Err(DecisionError::InvalidResponse(format!(
                "expected {} answers but received {}",
                questions.len(),
                answers.len()
            )));
        }
        for (question, answer) in questions.iter().zip(&answers) {
            if answer.name != question.name {
                return Err(DecisionError::InvalidResponse(format!(
                    "expected an answer to `{}` but received `{}`",
                    question.name, answer.name
                )));
            }
            let matches = match (&question.kind, &answer.outcome) {
                (_, Outcome::Refusal) | (QuestionKind::Predicate, Outcome::Predicate { .. }) => {
                    true
                }
                (
                    QuestionKind::Choice { choices },
                    Outcome::Choice {
                        choice,
                        probabilities,
                        ..
                    },
                ) => {
                    let offered =
                        |value: &ChoiceValue| choices.iter().any(|option| option.value == *value);
                    offered(choice) && probabilities.iter().all(|entry| offered(&entry.value))
                }
                (QuestionKind::Score { levels }, Outcome::Score { probabilities, .. }) => {
                    probabilities.iter().all(|entry| {
                        usize::try_from(entry.index).is_ok_and(|index| index < levels.len())
                    })
                }
                _ => false,
            };
            if !matches {
                return Err(DecisionError::InvalidResponse(format!(
                    "the answer to `{}` does not match its question",
                    question.name
                )));
            }
        }
        Ok(Self {
            model: model.into(),
            answers,
            usage,
        })
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn answers(&self) -> &[Answer] {
        &self.answers
    }

    #[must_use]
    pub fn answer(&self, name: &str) -> Option<&Answer> {
        self.answers.iter().find(|answer| answer.name == name)
    }

    #[must_use]
    pub const fn usage(&self) -> Option<DecisionUsage> {
        self.usage
    }

    #[must_use]
    pub fn into_answers(self) -> Vec<Answer> {
        self.answers
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Answer {
    pub name: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Predicate {
        probability: f64,
    },
    Choice {
        choice: ChoiceValue,
        confidence: f64,
        probabilities: Vec<ChoiceProbability>,
    },
    Score {
        score: f64,
        confidence: f64,
        probabilities: Vec<LevelProbability>,
    },
    Refusal,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChoiceProbability {
    pub value: ChoiceValue,
    pub probability: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LevelProbability {
    pub index: u32,
    pub label: String,
    pub probability: f64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DecisionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}
