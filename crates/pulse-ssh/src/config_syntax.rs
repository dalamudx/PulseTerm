use super::{ConfigError, MAX_LINES, MAX_SOURCE, MAX_TOTAL};
pub(super) enum Directive {
    Host(Vec<String>),
    Match(Vec<Criterion>),
    Include(Vec<String>),
    HostName(String),
    Port(u16),
    User(String),
    IdentityFile(String),
    IdentitiesOnly(bool),
    Unsupported,
}
pub(super) enum Criterion {
    All,
    Host(Vec<String>),
    OriginalHost(Vec<String>),
    User(Vec<String>),
    LocalUser(Vec<String>),
    Unsupported,
}
pub(super) fn parse(
    bytes: &[u8],
    total: &mut usize,
    lines: &mut usize,
) -> Result<Vec<Directive>, ConfigError> {
    *total = total.checked_add(bytes.len()).ok_or(ConfigError::Limit)?;
    if bytes.len() > MAX_SOURCE || *total > MAX_TOTAL {
        return Err(ConfigError::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ConfigError::Syntax)?;
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(ConfigError::Syntax);
    }
    let mut directives = Vec::new();
    for line in text.lines() {
        *lines += 1;
        if *lines > MAX_LINES || line.len() > 8192 {
            return Err(ConfigError::Limit);
        }
        let line = line.trim_ascii();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let end = line
            .find(|c: char| c.is_ascii_whitespace() || c == '=')
            .unwrap_or(line.len());
        let key = &line[..end];
        if key.is_empty() || key.len() > 64 || !key.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(ConfigError::Syntax);
        }
        let mut rest = line[end..].trim_ascii_start();
        if let Some(after) = rest.strip_prefix('=') {
            rest = after.trim_ascii_start();
        }
        let words = words(rest)?;
        if words.is_empty() {
            return Err(ConfigError::Syntax);
        }
        let directive = match key.to_ascii_lowercase().as_str() {
            "host" => Directive::Host(patterns(words, false)?),
            "match" => Directive::Match(criteria(words)?),
            "include" => {
                for path in &words {
                    if !path.is_ascii() || path.len() > 512 || path.contains(['[', ']', '%', '~']) {
                        return Err(ConfigError::Unsupported);
                    }
                }
                Directive::Include(words)
            }
            "hostname" => Directive::HostName(single(words)?),
            "user" => Directive::User(single(words)?),
            "port" => {
                let raw = single(words)?;
                if !raw.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(ConfigError::Syntax);
                }
                let port = raw.parse::<u16>().map_err(|_| ConfigError::Syntax)?;
                if port == 0 {
                    return Err(ConfigError::Syntax);
                }
                Directive::Port(port)
            }
            "identityfile" => Directive::IdentityFile(single(words)?),
            "identitiesonly" => {
                Directive::IdentitiesOnly(match single(words)?.to_ascii_lowercase().as_str() {
                    "yes" => true,
                    "no" => false,
                    _ => return Err(ConfigError::Syntax),
                })
            }
            _ => Directive::Unsupported,
        };
        directives.push(directive);
    }
    Ok(directives)
}
fn single(mut words: Vec<String>) -> Result<String, ConfigError> {
    if words.len() != 1 {
        return Err(ConfigError::Syntax);
    }
    words.pop().ok_or(ConfigError::Syntax)
}
fn words(text: &str) -> Result<Vec<String>, ConfigError> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quoted: Option<char> = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' if quoted.is_none() || quoted == Some(c) => {
                quoted = if quoted.is_none() { Some(c) } else { None };
                started = true;
            }
            '\\' => {
                if quoted == Some('\'') {
                    return Err(ConfigError::Unsupported);
                }
                let escaped = chars.next().ok_or(ConfigError::Syntax)?;
                if !matches!(escaped, '"' | '\'' | '\\' | ' ' | '\t' | '#') {
                    return Err(ConfigError::Unsupported);
                }
                word.push(escaped);
                started = true;
            }
            '#' if quoted.is_none() && !started => break,
            c if c.is_ascii_whitespace() && quoted.is_none() => {
                if started {
                    if word.is_empty() {
                        return Err(ConfigError::Syntax);
                    }
                    result.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
        if word.len() > 1024 || result.len() > 32 {
            return Err(ConfigError::Limit);
        }
    }
    if quoted.is_some() {
        return Err(ConfigError::Syntax);
    }
    if started {
        if word.is_empty() {
            return Err(ConfigError::Syntax);
        }
        result.push(word);
    }
    if result.len() > 32 {
        return Err(ConfigError::Limit);
    }
    Ok(result)
}
fn patterns(words: Vec<String>, comma: bool) -> Result<Vec<String>, ConfigError> {
    let mut result = Vec::new();
    for word in words {
        for pattern in if comma {
            word.split(',').collect::<Vec<_>>()
        } else {
            vec![word.as_str()]
        } {
            let value = pattern.strip_prefix('!').unwrap_or(pattern);
            if value.is_empty()
                || value.len() > 512
                || !value.is_ascii()
                || value.bytes().any(|b| b.is_ascii_whitespace())
                || value.contains(['!', '[', ']', ',', '%'])
            {
                return Err(ConfigError::Unsupported);
            }
            result.push(pattern.to_owned());
            if result.len() > 32 {
                return Err(ConfigError::Limit);
            }
        }
    }
    Ok(result)
}
fn criteria(words: Vec<String>) -> Result<Vec<Criterion>, ConfigError> {
    if words.len() == 1 && words[0].eq_ignore_ascii_case("all") {
        return Ok(vec![Criterion::All]);
    }
    let mut values = words.into_iter();
    let mut result = Vec::new();
    while let Some(key) = values.next() {
        let kind = key.to_ascii_lowercase();
        if !matches!(
            kind.as_str(),
            "host" | "originalhost" | "user" | "localuser"
        ) {
            return Ok(vec![Criterion::Unsupported]);
        }
        let value = values.next().ok_or(ConfigError::Syntax)?;
        let patterns = patterns(vec![value], true)?;
        result.push(match kind.as_str() {
            "host" => Criterion::Host(patterns),
            "originalhost" => Criterion::OriginalHost(patterns),
            "user" => Criterion::User(patterns),
            _ => Criterion::LocalUser(patterns),
        });
    }
    Ok(result)
}
pub(super) fn pattern_list(
    patterns: &[String],
    value: &str,
    lower: bool,
    budget: &mut usize,
) -> Result<bool, ConfigError> {
    let mut positive = false;
    for pattern in patterns {
        if let Some(negated) = pattern.strip_prefix('!') {
            if wildcard(negated, value, lower, budget)? {
                return Ok(false);
            }
        } else {
            positive |= wildcard(pattern, value, lower, budget)?;
        }
    }
    Ok(positive)
}
pub(super) fn wildcard(
    pattern: &str,
    value: &str,
    lower: bool,
    budget: &mut usize,
) -> Result<bool, ConfigError> {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        *budget = budget.checked_sub(1).ok_or(ConfigError::Limit)?;
        if p < pattern.len()
            && (pattern[p] == b'?'
                || if lower {
                    pattern[p].eq_ignore_ascii_case(&value[v])
                } else {
                    pattern[p] == value[v]
                })
        {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(star) = star {
            p = star + 1;
            retry += 1;
            v = retry;
        } else {
            return Ok(false);
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        *budget = budget.checked_sub(1).ok_or(ConfigError::Limit)?;
        p += 1;
    }
    Ok(p == pattern.len())
}

pub(super) fn path_pattern(
    pattern: &str,
    name: &str,
    budget: &mut usize,
) -> Result<bool, ConfigError> {
    let mut names = name.split('/');
    for part in pattern.split('/') {
        let Some(name) = names.next() else {
            return Ok(false);
        };
        if (name.starts_with('.') && !part.starts_with('.'))
            || !wildcard(part, name, false, budget)?
        {
            return Ok(false);
        }
    }
    Ok(names.next().is_none())
}
