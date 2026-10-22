//! Count physical Rust code lines while preserving literal and comment boundaries.

/// Count nonblank lines outside comments, including nonblank string contents.
pub fn code_lines(source: &str) -> usize {
    let source = source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .replace("\r\n", "\n");
    let characters: Vec<char> = source.chars().collect();
    let mut count = 0;
    let mut line_has_code = false;
    let mut index = 0;
    let mut block_depth = 0;
    let mut in_string = false;
    let mut raw_end: Option<Vec<char>> = None;
    let mut escaped = false;
    while index < characters.len() {
        let character = characters[index];
        let following = characters.get(index..index + 2);
        if character == '\n' {
            count += usize::from(line_has_code);
            line_has_code = false;
            escaped = false;
            index += 1;
            continue;
        }

        if in_string || raw_end.is_some() {
            if let Some(end) = &raw_end
                && characters[index..].starts_with(end)
            {
                line_has_code = true;
                index += end.len();
                raw_end = None;
                continue;
            }

            if in_string && escaped {
                escaped = false;
            } else if in_string && character == '\\' {
                escaped = true;
            } else if in_string && character == '"' {
                line_has_code = true;
                index += 1;
                in_string = false;
                continue;
            }

            line_has_code |= !character.is_whitespace();
            index += 1;
            continue;
        }

        if block_depth > 0 {
            if following == Some(&['/', '*']) {
                block_depth += 1;
                index += 2;
            } else if following == Some(&['*', '/']) {
                block_depth -= 1;
                index += 2;
            } else {
                index += 1;
            }

            continue;
        }

        if following == Some(&['/', '/']) {
            while index < characters.len() && characters[index] != '\n' {
                index += 1;
            }

            continue;
        }

        if following == Some(&['/', '*']) {
            block_depth = 1;
            index += 2;
            continue;
        }

        let prefix = match characters[index..] {
            ['b' | 'c', 'r', ..] => 2,
            ['r', ..] => 1,
            _ => 0,
        };
        if prefix > 0 {
            let mut quote = index + prefix;
            while characters.get(quote) == Some(&'#') {
                quote += 1;
            }

            if characters.get(quote) == Some(&'"') {
                line_has_code = true;
                let mut end = vec!['"'];
                end.extend(std::iter::repeat_n('#', quote - index - prefix));
                raw_end = Some(end);
                index = quote + 1;
                continue;
            }
        }

        if character == '"' {
            line_has_code = true;
            in_string = true;
            index += 1;
            continue;
        }

        if character == '\'' {
            line_has_code = true;
            index = character_end(&characters, index).unwrap_or(index + 1);
            continue;
        }

        line_has_code |= !character.is_whitespace();
        index += 1;
    }

    count + usize::from(line_has_code)
}

// A quote begins either a character literal or a lifetime such as 'a.
fn character_end(characters: &[char], start: usize) -> Option<usize> {
    let mut closing = start + 1;
    if characters.get(closing) == Some(&'\\') {
        closing += 1;
        match characters.get(closing) {
            Some('u') if characters.get(closing + 1) == Some(&'{') => {
                closing = characters[closing + 2..]
                    .iter()
                    .position(|value| *value == '}')?
                    + closing
                    + 3;
            }
            Some('x') => closing += 3,
            Some(_) => closing += 1,
            None => return None,
        }
    } else {
        closing += 1;
    }

    (characters.get(closing) == Some(&'\'')).then_some(closing + 1)
}
