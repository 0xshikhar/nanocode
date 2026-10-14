use crate::BrowserVoiceEffects;
use serde_json::Value;
use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
};

#[derive(Default)]
pub(crate) struct BrowserSpeechDelivery {
    generation: u64,
    voice_input: bool,
    input_caption: Option<u64>,
    output_caption: Option<(u64, bool)>,
    last_input: Option<u64>,
    owner: Option<u64>,
    candidate: Option<String>,
    pending: VecDeque<(u64, String)>,
    suppressed: bool,
}

impl BrowserSpeechDelivery {
    pub(crate) fn new() -> Self {
        Self {
            voice_input: true,
            ..Self::default()
        }
    }
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn accepts_caption(&self) -> bool {
        self.voice_input
            && self
                .output_caption
                .is_none_or(|(generation, _)| generation == self.generation)
    }

    pub(crate) const fn allow_explicit_speech(&mut self) {
        self.voice_input = true;
        self.suppressed = false;
        self.output_caption = None;
    }

    pub(crate) const fn playback_enabled(&self) -> bool {
        !self.suppressed
    }

    pub(crate) fn invalidate(&mut self) -> BrowserVoiceEffects {
        self.generation = self.generation.wrapping_add(1);
        self.voice_input = false;
        self.suppressed = true;
        BrowserVoiceEffects {
            input_generation: Some(self.generation),
            playback_enabled: Some(false),
            undelivered_answers: self.pending.drain(..).map(|(_, text)| text).collect(),
            ..BrowserVoiceEffects::default()
        }
    }

    pub(crate) fn close(&mut self) -> BrowserVoiceEffects {
        self.owner = None;
        self.candidate = None;
        self.invalidate()
    }

    pub(crate) fn realtime(&mut self, event: &Value) -> BrowserVoiceEffects {
        let mut effects = BrowserVoiceEffects::default();
        match event["type"].as_str().unwrap_or_default() {
            "input_transcript.added" if self.input_caption.is_none() => {
                effects = self.invalidate();
                self.voice_input = true;
                self.input_caption = Some(self.generation);
            }
            "output_transcript.added" => {
                if event["item"]["text"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
                {
                    self.output_caption
                        .get_or_insert((self.generation, !self.pending.is_empty()));
                    if self.voice_input
                        && self
                            .output_caption
                            .is_some_and(|(generation, _)| generation == self.generation)
                    {
                        self.suppressed = false;
                        effects.playback_enabled = Some(true);
                    }
                }
            }
            "turn.done" => {
                let text = event["turn"]["transcript"].as_str().unwrap_or_default();
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                for word in text.split_whitespace() {
                    word.hash(&mut hash);
                }
                let fingerprint = Some(hash.finish());
                match event["turn"]["role"].as_str() {
                    Some("user") => match self.input_caption.take() {
                        Some(generation) if generation == self.generation => {
                            self.voice_input = !text.trim().is_empty();
                            self.last_input = fingerprint;
                            if !self.voice_input {
                                self.suppressed = false;
                                effects.playback_enabled = Some(true);
                            }
                        }
                        None if !text.trim().is_empty()
                            && (self.voice_input || fingerprint != self.last_input) =>
                        {
                            effects = self.invalidate();
                            self.voice_input = true;
                            self.last_input = fingerprint;
                        }
                        _ => {}
                    },
                    Some("assistant") => {
                        if self.output_caption == Some((self.generation, true))
                            && self.voice_input
                            && let Some(index) =
                                self.pending.iter().position(|(generation, answer)| {
                                    *generation == self.generation
                                        && answer.split_whitespace().eq(text.split_whitespace())
                                })
                        {
                            self.pending.remove(index);
                        }
                        self.output_caption = None;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        effects
    }

    pub(crate) fn delegate(&mut self) {
        self.owner = self.voice_input.then_some(self.generation);
        self.candidate = None;
    }

    pub(crate) fn agent(&mut self, event: &Value, silent: bool) -> BrowserVoiceEffects {
        let mut effects = BrowserVoiceEffects {
            input_generation: Some(self.generation),
            ..BrowserVoiceEffects::default()
        };
        match event["type"].as_str().unwrap_or_default() {
            "run.started" if self.owner.is_none() => {
                effects = self.invalidate();
            }
            "assistant.message" if self.owner.is_some() => {
                let text = event["payload"]["text"].as_str().unwrap_or_default();
                let phase = event["payload"]["phase"].as_str();
                if !text.trim().is_empty()
                    && phase != Some("commentary")
                    && (phase == Some("final_answer")
                        || !(text.trim_start().starts_with("[ANALYSIS]")
                            || text.trim_start().starts_with("[COMMENTARY]")))
                {
                    self.candidate = Some(text.to_owned());
                }
            }
            "run.completed" => {
                let owner = self.owner.take();
                if let Some(text) = self.candidate.take() {
                    let text = text
                        .trim()
                        .strip_prefix("[FINAL]")
                        .unwrap_or(text.trim())
                        .trim()
                        .to_owned();
                    if text.is_empty() {
                        return effects;
                    }
                    if silent
                        || !self.voice_input
                        || owner != Some(self.generation)
                        || text.len() > 990 * 4
                    {
                        effects.undelivered_answers.push(text);
                    } else {
                        if self.pending.len() == 16
                            && let Some((_, text)) = self.pending.pop_front()
                        {
                            effects.undelivered_answers.push(text);
                        }
                        self.pending.push_back((self.generation, text.clone()));
                        effects.frames = crate::browser::session_context_frames(&text, "speakable")
                            .into_iter()
                            .map(|frame| frame.to_string())
                            .collect();
                        effects.acknowledge_frames = true;
                    }
                }
            }
            "run.failed" | "run.cancelled" | "turn_failed" => {
                let owned = self.owner.take().is_some();
                self.candidate = None;
                effects = self.invalidate();
                if owned && event["type"] != "run.cancelled" {
                    effects
                        .undelivered_answers
                        .push("The coding agent could not complete the request.".into());
                }
            }
            _ => {}
        }
        effects
    }
}
