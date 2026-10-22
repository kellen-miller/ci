//! Select CI groups using complete, NUL-delimited Git diffs.

use std::collections::BTreeMap;
use std::env;
use std::process::Command;

use anyhow::{Result, ensure};

use crate::{command, github_output};

pub type Filters = BTreeMap<String, Vec<String>>;

/// Match Python fnmatch-style globs, where a star also includes slashes.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn select(filters: &Filters, paths: &[String]) -> Result<BTreeMap<String, bool>> {
    ensure!(
        !filters.is_empty(),
        "filters must be a nonempty JSON object"
    );
    let mut selected = BTreeMap::new();
    for (group, patterns) in filters {
        ensure!(
            !group.is_empty() && !patterns.is_empty(),
            "{group}: provide a nonempty list of glob patterns"
        );
        let mut matched = false;
        for pattern in patterns {
            ensure!(
                !pattern.is_empty() && !pattern.starts_with('!'),
                "{group}: only positive, nonempty glob patterns are supported"
            );
            for alternative in std::iter::once(pattern.as_str()).chain(pattern.strip_prefix("**/"))
            {
                let matcher = PathPattern::new(alternative);
                matched |= paths.iter().any(|path| matcher.matches(path));
            }
        }

        selected.insert(group.clone(), matched);
    }

    Ok(selected)
}

/// Read the event contract and append selected groups to GITHUB_OUTPUT.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn run() -> Result<()> {
    let filters: Filters = serde_json::from_str(&env::var("FILTERS")?)?;
    // Validate even on non-PR events.
    let mut selected = select(&filters, &[])?;
    if env::var("EVENT_NAME")? == "pull_request" {
        let base = env::var("BASE_SHA")?;
        let head = env::var("HEAD_SHA")?;
        ensure!(
            !base.is_empty() && !head.is_empty(),
            "PR base and head SHAs are required"
        );
        let diff = command::capture_bytes(Command::new("git").args([
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            &format!("{base}...{head}"),
            "--",
        ]))?;
        let paths = diff
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| String::from_utf8(path.to_vec()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        selected = select(&filters, &paths)?;
    } else {
        selected.values_mut().for_each(|value| *value = true);
    }

    let changes = serde_json::to_string(&selected)?;
    github_output::append("changes", &changes)?;
    println!("CI groups: {changes}");
    Ok(())
}

// fnmatch treats unmatched brackets literally and permits stars across slashes.
// Parse character classes directly; filesystem glob parsers use different rules.
struct CharacterClass {
    negated: bool,
    ranges: Vec<(char, char)>,
}

enum Token {
    Star,
    Any,
    Literal(char),
    Class(CharacterClass),
}

struct PathPattern {
    tokens: Vec<Token>,
}

impl PathPattern {
    fn new(pattern: &str) -> Self {
        let characters: Vec<char> = pattern.chars().collect();
        let mut tokens = Vec::new();
        let mut index = 0;
        while index < characters.len() {
            let character = characters[index];
            index += 1;
            match character {
                '*' => tokens.push(Token::Star),
                '?' => tokens.push(Token::Any),
                '[' => {
                    let start = index;
                    let negated = characters.get(index) == Some(&'!');
                    let content = start + usize::from(negated);
                    let search = content + usize::from(characters.get(content) == Some(&']'));
                    let closing = characters
                        .iter()
                        .enumerate()
                        .skip(search)
                        .find(|(_, character)| **character == ']');
                    if let Some((closing, _)) = closing {
                        tokens.push(Token::Class(CharacterClass::parse(
                            &characters[content..closing],
                            negated,
                        )));
                        index = closing + 1;
                    } else {
                        tokens.push(Token::Literal('['));
                    }
                }

                _ => tokens.push(Token::Literal(character)),
            }
        }

        Self { tokens }
    }

    fn matches(&self, path: &str) -> bool {
        let characters: Vec<char> = path.chars().collect();
        let mut previous = vec![false; characters.len() + 1];
        previous[0] = true;
        for token in &self.tokens {
            let mut current = vec![false; previous.len()];
            if matches!(token, Token::Star) {
                current[0] = previous[0];
            }

            for (index, character) in characters.iter().enumerate() {
                current[index + 1] = match token {
                    Token::Star => previous[index + 1] || current[index],
                    Token::Any => previous[index],
                    Token::Literal(expected) => previous[index] && character == expected,
                    Token::Class(class) => {
                        previous[index]
                            && (class
                                .ranges
                                .iter()
                                .any(|(low, high)| low <= character && character <= high)
                                != class.negated)
                    }
                };
            }

            previous = current;
        }

        previous[characters.len()]
    }
}

impl CharacterClass {
    fn parse(characters: &[char], negated: bool) -> Self {
        let mut ranges = Vec::new();
        let mut index = 0;
        while index < characters.len() {
            let low = characters[index];
            if characters.get(index + 1) == Some(&'-') && index + 2 < characters.len() {
                let high = characters[index + 2];
                if low <= high {
                    ranges.push((low, high));
                }

                index += 3;
            } else {
                ranges.push((low, low));
                index += 1;
            }
        }

        Self { negated, ranges }
    }
}

#[cfg(test)]
mod tests {
    use super::PathPattern;

    #[test]
    fn fnmatch_handles_unicode_newlines_slashes_and_literal_brackets() {
        for (pattern, path, expected) in [
            ("*", "nested/file\n.rs", true),
            ("??", "é界", true),
            ("[abc", "[abc", true),
            ("[abc*", "[abc/def", true),
            ("[]]", "]", true),
            ("[!]]", "]", false),
            ("[z-a]", "z", false),
            ("[!z-a]", "z", true),
            ("[a-b-c]", "-", true),
            ("[a--c]", "c", true),
            ("[^a]", "^", true),
            ("[[]", "[", true),
            ("***.rs", "src/lib.rs", true),
            ("*", ".hidden", true),
        ] {
            assert_eq!(
                PathPattern::new(pattern).matches(path),
                expected,
                "{pattern:?} against {path:?}"
            );
        }
    }
}
