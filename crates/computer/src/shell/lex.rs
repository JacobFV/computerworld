#[derive(Clone, Debug)]
pub(super) enum Token {
    Word(Vec<(String, u8)>),
    Op(String),
}
pub(super) fn lex(s: &str, ps: bool) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    let mut parts = vec![];
    let mut word = false;
    // Index at which the previous word ended, so `2>` is distinguished from `2 >`.
    let mut word_end = usize::MAX;
    let mut heredocs: Vec<(usize, String, u8, bool)> = Vec::new();
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' || ch == '"' {
            word = true;
            let q = ch;
            i += 1;
            let mut text = String::new();
            while i < chars.len() && chars[i] != q {
                if q == '"' && chars[i] == '$' && chars.get(i + 1) == Some(&'(') {
                    let end = balanced_end(&chars, i + 1)?;
                    text.extend(chars[i..end].iter());
                    i = end;
                    continue;
                }
                if chars[i] == if ps { '`' } else { '\\' } && q == '"' && i + 1 < chars.len() {
                    text.push(chars[i]);
                    i += 1;
                }
                text.push(chars[i]);
                i += 1;
            }
            if i == chars.len() {
                return Err("unterminated quote".into());
            }
            parts.push((text, u8::from(q == '"')));
            i += 1;
            continue;
        }
        if ch == '`' && !ps {
            word = true;
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '`' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                i += 1;
            }
            if i == chars.len() {
                return Err("unterminated backquote".into());
            }
            let body: String = chars[start..i].iter().collect();
            parts.push((format!("$({body})"), 3));
            i += 1;
            continue;
        }
        if ch == if ps { '`' } else { '\\' } {
            word = true;
            i += 1;
            if i < chars.len() {
                parts.push((chars[i].to_string(), 0));
                i += 1;
            }
            continue;
        }
        if ch == '#' && !word {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == ' ' || ch == '\t' || ";\n|&<>()".contains(ch) {
            if word {
                out.push(Token::Word(std::mem::take(&mut parts)));
                word = false;
                word_end = i;
            }
            if ch == ' ' || ch == '\t' {
                i += 1;
                continue;
            }
            if ch == '<' && chars.get(i + 1) == Some(&'<') {
                i += 2;
                let strip = chars.get(i) == Some(&'-');
                if strip {
                    i += 1;
                }
                while chars.get(i).is_some_and(|c| *c == ' ' || *c == '\t') {
                    i += 1;
                }
                let quote = chars.get(i).copied().filter(|c| *c == '\'' || *c == '"');
                if quote.is_some() {
                    i += 1;
                }
                let start = i;
                while i < chars.len()
                    && if let Some(q) = quote {
                        chars[i] != q
                    } else {
                        !chars[i].is_whitespace() && !";&|<>".contains(chars[i])
                    }
                {
                    i += 1;
                }
                let delimiter: String = chars[start..i].iter().collect();
                if delimiter.is_empty() {
                    return Err("missing heredoc delimiter".into());
                }
                if quote.is_some() {
                    if i == chars.len() {
                        return Err("unterminated heredoc delimiter".into());
                    }
                    i += 1;
                }
                out.push(Token::Op("<<".into()));
                let index = out.len();
                out.push(Token::Word(vec![(String::new(), 0)]));
                heredocs.push((index, delimiter, u8::from(quote.is_none()), strip));
                continue;
            }
            if ch == '&' && chars.get(i + 1) == Some(&'>') {
                let mut op = "&>".to_string();
                i += 2;
                if chars.get(i) == Some(&'>') {
                    op.push('>');
                    i += 1;
                }
                out.push(Token::Op(op));
                continue;
            }
            let start = i;
            let mut op = ch.to_string();
            if i + 1 < chars.len() && ((ch == '|' || ch == '&' || ch == '>') && chars[i + 1] == ch)
            {
                op.push(ch);
                i += 1;
            }
            if ch == '>' {
                // A descriptor prefix only counts when glued on: `echo 2 > f` redirects stdout.
                if let Some(Token::Word(v)) = out.last() {
                    if word_end == start && v.len() == 1 && (v[0].0 == "1" || v[0].0 == "2") {
                        let fd = v[0].0.clone();
                        out.pop();
                        op = format!("{fd}{op}");
                    }
                }
                // `2>&1` duplicates a descriptor and takes no path operand.
                if chars.get(i + 1) == Some(&'&')
                    && chars.get(i + 2).is_some_and(char::is_ascii_digit)
                {
                    op.push('&');
                    op.push(chars[i + 2]);
                    i += 2;
                }
            }
            out.push(Token::Op(op));
            i += 1;
            if ch == '\n' {
                for (index, delimiter, flags, strip) in heredocs.drain(..) {
                    let mut body = String::new();
                    let mut found = false;
                    while i < chars.len() {
                        let start = i;
                        while i < chars.len() && chars[i] != '\n' {
                            i += 1;
                        }
                        let raw: String = chars[start..i].iter().collect();
                        if i < chars.len() {
                            i += 1;
                        }
                        let line = if strip {
                            raw.trim_start_matches('\t')
                        } else {
                            &raw
                        };
                        if line == delimiter {
                            found = true;
                            break;
                        }
                        body.push_str(line);
                        body.push('\n');
                    }
                    if !found {
                        return Err("unterminated heredoc".into());
                    }
                    out[index] = Token::Word(vec![(body, flags)]);
                }
            }
            continue;
        }
        word = true;
        let mut text = String::new();
        while i < chars.len()
            && !" \t\n;|&<>()\"'".contains(chars[i])
            && chars[i] != if ps { '`' } else { '\\' }
            && !(chars[i] == '`' && !ps)
        {
            if chars[i] == '$' && chars.get(i + 1) == Some(&'(') {
                let end = balanced_end(&chars, i + 1)?;
                text.extend(chars[i..end].iter());
                i = end;
            } else {
                text.push(chars[i]);
                i += 1;
            }
        }
        parts.push((text, 3));
    }
    if word {
        out.push(Token::Word(parts));
    }
    if !heredocs.is_empty() {
        return Err("heredoc requires newline and body".into());
    }
    Ok(out)
}
pub(super) fn balanced_end(chars: &[char], start: usize) -> Result<usize, String> {
    let mut depth = 1;
    let mut i = start + 1;
    let mut quote = None;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            i += 2;
            continue;
        }
        if let Some(q) = quote {
            if q == ch {
                quote = None
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch)
        } else if ch == '(' {
            depth += 1;
            if depth > 32 {
                return Err("substitution nesting exceeds 32".into());
            }
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                return Ok(i + 1);
            }
        }
        i += 1;
    }
    Err("unterminated substitution".into())
}
