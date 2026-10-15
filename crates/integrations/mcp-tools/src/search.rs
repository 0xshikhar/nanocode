use std::collections::HashMap;

const K1: f32 = 1.2;
const B: f32 = 0.75;

pub(super) struct SearchEngine {
    terms: HashMap<String, Term>,
}

struct Term {
    idf: f32,
    postings: Vec<(usize, f32)>,
}

impl SearchEngine {
    pub(super) fn new(documents: impl IntoIterator<Item = Vec<String>>) -> Self {
        let documents = documents.into_iter().collect::<Vec<_>>();
        let mut terms = HashMap::<String, Term>::new();
        if documents.is_empty() {
            return Self { terms };
        }
        let total_length = documents
            .iter()
            .map(|tokens| tokens.len() as u64)
            .sum::<u64>();
        let average_length = (total_length as f64 / documents.len() as f64) as f32;
        for (index, tokens) in documents.iter().enumerate() {
            let mut frequencies = HashMap::<&str, usize>::new();
            for token in tokens {
                *frequencies.entry(token).or_default() += 1;
            }
            for (token, frequency) in frequencies {
                let frequency = frequency as f32;
                let normalization = 1.0 - B + B * (tokens.len() as f32 / average_length);
                let weight = frequency * (K1 + 1.0) / (frequency + K1 * normalization);
                terms
                    .entry(token.to_owned())
                    .or_insert_with(|| Term {
                        idf: 0.0,
                        postings: Vec::new(),
                    })
                    .postings
                    .push((index, weight));
            }
        }
        for term in terms.values_mut() {
            let frequency = term.postings.len() as f32;
            term.idf = (1.0 + (documents.len() as f32 - frequency + 0.5) / (frequency + 0.5)).ln();
        }
        Self { terms }
    }

    pub(super) fn search(&self, query: &[String], limit: usize) -> Vec<(usize, f32)> {
        let mut scores = HashMap::<usize, f32>::new();
        for token in query {
            if let Some(term) = self.terms.get(token) {
                for &(index, weight) in &term.postings {
                    *scores.entry(index).or_default() += term.idf * weight;
                }
            }
        }
        let mut results = scores.into_iter().collect::<Vec<_>>();
        results.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        results.truncate(limit);
        results
    }
}
pub(super) fn tokenize(input_text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut previous = None;
    let mut characters = input_text.chars().peekable();

    while let Some(character) = characters.next() {
        if !character.is_alphanumeric() {
            push_search_token(&mut tokens, &mut token);
            previous = None;
            continue;
        }

        let starts_word = !token.is_empty()
            && character.is_uppercase()
            && (previous.is_some_and(char::is_lowercase)
                || characters.peek().is_some_and(|next| next.is_lowercase()));
        if starts_word {
            push_search_token(&mut tokens, &mut token);
        }
        token.extend(character.to_lowercase());
        previous = Some(character);
    }
    push_search_token(&mut tokens, &mut token);
    tokens
}

fn push_search_token(tokens: &mut Vec<String>, token: &mut String) {
    if !token.is_empty() {
        tokens.push(std::mem::take(token));
    }
}
