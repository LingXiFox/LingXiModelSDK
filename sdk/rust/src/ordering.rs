use std::cmp::Ordering;
use std::collections::HashMap;

use crate::types::{CatalogModel, CatalogProvider};

#[derive(Debug, Clone, PartialEq, Eq)]
enum NaturalToken {
    Number { trimmed: String, raw: String },
    Text(String),
}

fn tokenize_natural(text: &str) -> Vec<NaturalToken> {
    let mut tokens = Vec::new();
    let mut digits = String::new();
    let mut words = String::new();

    let flush_digits = |digits: &mut String, tokens: &mut Vec<NaturalToken>| {
        if digits.is_empty() {
            return;
        }
        let raw = std::mem::take(digits);
        let trimmed = raw.trim_start_matches('0');
        let trimmed = if trimmed.is_empty() { "0" } else { trimmed }.to_string();
        tokens.push(NaturalToken::Number { trimmed, raw });
    };

    let flush_words = |words: &mut String, tokens: &mut Vec<NaturalToken>| {
        if words.is_empty() {
            return;
        }
        let w = std::mem::take(words);
        tokens.push(NaturalToken::Text(w.to_lowercase()));
    };

    for ch in text.chars() {
        if ch.is_ascii_digit() {
            flush_words(&mut words, &mut tokens);
            digits.push(ch);
        } else {
            flush_digits(&mut digits, &mut tokens);
            words.push(ch);
        }
    }
    flush_digits(&mut digits, &mut tokens);
    flush_words(&mut words, &mut tokens);
    tokens
}

pub fn compare_natural(a: &str, b: &str) -> Ordering {
    let left = tokenize_natural(a);
    let right = tokenize_natural(b);
    for (lt, rt) in left.iter().zip(right.iter()) {
        let ord = match (lt, rt) {
            (NaturalToken::Number { .. }, NaturalToken::Text(_)) => Ordering::Less,
            (NaturalToken::Text(_), NaturalToken::Number { .. }) => Ordering::Greater,
            (
                NaturalToken::Number {
                    trimmed: lt_t,
                    raw: lt_r,
                },
                NaturalToken::Number {
                    trimmed: rt_t,
                    raw: rt_r,
                },
            ) => lt_t
                .len()
                .cmp(&rt_t.len())
                .then_with(|| lt_t.cmp(rt_t))
                .then_with(|| lt_r.cmp(rt_r)),
            (NaturalToken::Text(lt_s), NaturalToken::Text(rt_s)) => lt_s.cmp(rt_s),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    left.len().cmp(&right.len())
}

pub fn sort_providers(providers: &mut [CatalogProvider], featured: &[&str]) {
    let mut ranks = HashMap::with_capacity(featured.len());
    for (idx, id) in featured.iter().enumerate() {
        ranks.insert(id.trim().to_lowercase(), idx);
    }
    providers.sort_by(|a, b| {
        let ra = ranks
            .get(&a.id.trim().to_lowercase())
            .copied()
            .unwrap_or(usize::MAX);
        let rb = ranks
            .get(&b.id.trim().to_lowercase())
            .copied()
            .unwrap_or(usize::MAX);
        ra.cmp(&rb)
            .then_with(|| compare_natural(&a.name, &b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
}

pub fn sort_models(models: &mut [CatalogModel]) {
    models.sort_by(|a, b| {
        let by_date = match (&a.release_date, &b.release_date) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(la), Some(lb)) => lb.cmp(la),
        };
        by_date
            .then_with(|| {
                compare_natural(
                    a.family.as_deref().unwrap_or(""),
                    b.family.as_deref().unwrap_or(""),
                )
            })
            .then_with(|| compare_natural(&a.name, &b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
}
