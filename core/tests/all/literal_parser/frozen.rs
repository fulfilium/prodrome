//! The literal parser as it was before it read bytes instead of characters,
//! kept unchanged as the reference that `super` compares the parser with.
//! Only the names it reaches through `prodrome::literal`'s public items
//! differ from the code it was copied from.

use prodrome::literal::{
    Call, Datetime, Finite, Int, ProdromeError, Signature, Timedelta, Value, Vocabulary,
};

/// How deep a literal may nest before the parser refuses. CPython answers a
/// deeper one with `RecursionError`, which `parse_literal` turns into a
/// `ValueError`; a recursive-descent parser in Rust would answer with a stack
/// overflow, which is an abort and not a refusal — so the limit is the type of
/// the guarantee, not a tuning knob.
const MAX_DEPTH: usize = 128;

/// Parse exactly the grammar of §2 against `vocabulary`, and nothing else.
pub fn parse(text: &str, vocabulary: &dyn Vocabulary) -> Result<Value, ProdromeError> {
    let mut parser = Parser {
        input: text,
        pos: 0,
        vocabulary,
    };
    parser.skip_trivia();
    if parser.at_end() {
        return Err(ProdromeError::parse(
            "empty artifact, expected one expression",
        ));
    }
    let value = parser.expression(0)?;
    parser.skip_trivia();
    if !parser.at_end() {
        return Err(ProdromeError::parse(format!(
            "artifact must be a single expression, trailing text at byte {}",
            parser.pos
        )));
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    vocabulary: &'a dyn Vocabulary,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Whitespace and `#` comments, which CPython's tokenizer drops before the
    /// AST ever sees them — so a stored object may carry a comment line above
    /// its expression and still parse.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.pos += c.len_utf8();
                }
                Some('#') => {
                    while let Some(c) = self.peek() {
                        self.pos += c.len_utf8();
                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> Result<(), ProdromeError> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(ProdromeError::parse(format!(
                "expected {c:?} at byte {}, got {:?}",
                self.pos,
                self.peek()
            )))
        }
    }

    fn expression(&mut self, depth: usize) -> Result<Value, ProdromeError> {
        if depth > MAX_DEPTH {
            return Err(ProdromeError::parse("literal nests too deeply"));
        }
        self.skip_trivia();
        match self.peek() {
            None => Err(ProdromeError::parse(
                "expected an expression, got end of text",
            )),
            Some('-') => {
                self.bump();
                self.skip_trivia();
                // CPython's walker admits USub over an int/float CONSTANT and
                // nothing else: `-Flat(...)` is an operator, not a literal.
                match self.number()? {
                    Value::Int(int) => Ok(Value::Int(Int::new(!int.is_negative(), int.digits())?)),
                    Value::Float(f) => Ok(Value::Float(Finite::new(-f.get())?)),
                    other => Err(ProdromeError::parse(format!(
                        "unary minus applies to a number, got {other:?}"
                    ))),
                }
            }
            Some('\'') | Some('"') => self.string(),
            Some('(') => self.parenthesised(depth),
            Some('[') => self.bracketed(depth),
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == '_' => self.name_or_call(depth),
            Some(c) => Err(ProdromeError::parse(format!(
                "disallowed syntax at byte {}: {c:?}",
                self.pos
            ))),
        }
    }

    /// `(a, b)` is a tuple, `(a,)` a one-tuple, `()` the empty one — and `(a)`
    /// is just `a`, exactly as CPython parses it.
    fn parenthesised(&mut self, depth: usize) -> Result<Value, ProdromeError> {
        self.expect('(')?;
        self.skip_trivia();
        if self.eat(')') {
            return Ok(Value::Tuple(Vec::new()));
        }
        let first = self.expression(depth + 1)?;
        self.skip_trivia();
        if self.eat(')') {
            return Ok(first);
        }
        let mut items = vec![first];
        loop {
            self.expect(',')?;
            self.skip_trivia();
            if self.eat(')') {
                break;
            }
            items.push(self.expression(depth + 1)?);
            self.skip_trivia();
            if self.eat(')') {
                break;
            }
        }
        Ok(Value::Tuple(items))
    }

    /// CPython's walker maps a list display onto a tuple, so the grammar admits
    /// one and the print never produces one.
    fn bracketed(&mut self, depth: usize) -> Result<Value, ProdromeError> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_trivia();
        if self.eat(']') {
            return Ok(Value::Tuple(items));
        }
        loop {
            items.push(self.expression(depth + 1)?);
            self.skip_trivia();
            if self.eat(']') {
                break;
            }
            self.expect(',')?;
            self.skip_trivia();
            if self.eat(']') {
                break;
            }
        }
        Ok(Value::Tuple(items))
    }

    fn number(&mut self) -> Result<Value, ProdromeError> {
        let start = self.pos;
        let mut is_float = false;
        while let Some(c) = self.peek() {
            match c {
                '0'..='9' => {
                    self.bump();
                }
                '.' => {
                    is_float = true;
                    self.bump();
                }
                'e' | 'E' => {
                    // An exponent, but only when a sign or digit follows: `1e`
                    // is not a number and neither is the `e` of a name.
                    let mut lookahead = self.rest().chars();
                    lookahead.next();
                    match lookahead.next() {
                        Some(next) if next.is_ascii_digit() || next == '+' || next == '-' => {
                            is_float = true;
                            self.bump();
                            self.bump();
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        let text = &self.input[start..self.pos];
        if text.is_empty() || text == "." {
            return Err(ProdromeError::parse(format!(
                "expected a number at byte {start}"
            )));
        }
        if is_float {
            let parsed: f64 = text
                .parse()
                .map_err(|_| ProdromeError::parse(format!("malformed float {text:?}")))?;
            Ok(Value::Float(Finite::new(parsed)?))
        } else {
            Ok(Value::Int(Int::new(false, text)?))
        }
    }

    fn string(&mut self) -> Result<Value, ProdromeError> {
        let quote = self.bump().expect("caller peeked a quote");
        let mut out = String::new();
        loop {
            let c = self
                .bump()
                .ok_or_else(|| ProdromeError::parse("unterminated string literal"))?;
            if c == quote {
                return Ok(Value::Str(out));
            }
            if c == '\n' {
                return Err(ProdromeError::parse(
                    "newline in a single-quoted string literal",
                ));
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            let escape = self
                .bump()
                .ok_or_else(|| ProdromeError::parse("string ends in a backslash"))?;
            match escape {
                '\\' => out.push('\\'),
                '\'' => out.push('\''),
                '"' => out.push('"'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'a' => out.push('\u{7}'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'v' => out.push('\u{b}'),
                '0' => out.push('\0'),
                '\n' => {}
                'x' => out.push(self.escape_hex(2)?),
                'u' => out.push(self.escape_hex(4)?),
                'U' => out.push(self.escape_hex(8)?),
                other => {
                    return Err(ProdromeError::parse(format!(
                        "unknown string escape \\{other}"
                    )));
                }
            }
        }
    }

    fn escape_hex(&mut self, width: usize) -> Result<char, ProdromeError> {
        let start = self.pos;
        for _ in 0..width {
            match self.peek() {
                Some(c) if c.is_ascii_hexdigit() => {
                    self.bump();
                }
                _ => return Err(ProdromeError::parse("truncated \\x/\\u/\\U escape")),
            }
        }
        let point = u32::from_str_radix(&self.input[start..self.pos], 16)
            .map_err(|_| ProdromeError::parse("malformed \\x/\\u/\\U escape"))?;
        char::from_u32(point)
            .ok_or_else(|| ProdromeError::parse(format!("\\U{point:08x} is not a character")))
    }

    fn name_or_call(&mut self, depth: usize) -> Result<Value, ProdromeError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.bump();
            } else {
                break;
            }
        }
        let name = &self.input[start..self.pos];
        match name {
            "None" => return Ok(Value::None),
            "True" => return Ok(Value::Bool(true)),
            "False" => return Ok(Value::Bool(false)),
            _ => {}
        }
        self.skip_trivia();
        if self.peek() != Some('(') {
            return Err(ProdromeError::parse(format!(
                "disallowed bare name {name:?}"
            )));
        }
        let name = name.to_owned();
        let (positional, keywords) = self.arguments(depth)?;
        self.build_call(&name, positional, keywords)
    }

    #[allow(clippy::type_complexity)]
    fn arguments(
        &mut self,
        depth: usize,
    ) -> Result<(Vec<Value>, Vec<(String, Value)>), ProdromeError> {
        self.expect('(')?;
        let mut positional = Vec::new();
        let mut keywords: Vec<(String, Value)> = Vec::new();
        self.skip_trivia();
        if self.eat(')') {
            return Ok((positional, keywords));
        }
        loop {
            self.skip_trivia();
            if self.peek() == Some('*') {
                return Err(ProdromeError::parse(
                    "*args/**kwargs are not allowed in a call",
                ));
            }
            let keyword = self.keyword_name();
            match keyword {
                Some(key) => {
                    if keywords.iter().any(|(seen, _)| *seen == key) {
                        return Err(ProdromeError::parse(format!("field {key:?} given twice")));
                    }
                    let value = self.expression(depth + 1)?;
                    keywords.push((key, value));
                }
                None => {
                    if !keywords.is_empty() {
                        return Err(ProdromeError::parse("positional argument after a keyword"));
                    }
                    positional.push(self.expression(depth + 1)?);
                }
            }
            self.skip_trivia();
            if self.eat(')') {
                return Ok((positional, keywords));
            }
            self.expect(',')?;
            self.skip_trivia();
            if self.eat(')') {
                return Ok((positional, keywords));
            }
        }
    }

    /// `name=` at the cursor, consumed; nothing consumed otherwise. `==` is not
    /// a keyword and not in the grammar either way.
    fn keyword_name(&mut self) -> Option<String> {
        let start = self.pos;
        let mut end = self.pos;
        let mut chars = self.rest().char_indices();
        match chars.next() {
            Some((_, c)) if c.is_ascii_alphabetic() || c == '_' => end += c.len_utf8(),
            _ => return None,
        }
        for (_, c) in chars {
            if c.is_ascii_alphanumeric() || c == '_' {
                end += c.len_utf8();
            } else {
                break;
            }
        }
        let after = &self.input[end..];
        let trimmed = after.trim_start();
        let gap = after.len() - trimmed.len();
        if !trimmed.starts_with('=') || trimmed.starts_with("==") {
            return None;
        }
        let name = self.input[start..end].to_owned();
        self.pos = end + gap + 1;
        Some(name)
    }

    fn build_call(
        &self,
        name: &str,
        positional: Vec<Value>,
        keywords: Vec<(String, Value)>,
    ) -> Result<Value, ProdromeError> {
        match name {
            "datetime" => return build_datetime(positional, keywords),
            "timedelta" => return build_timedelta(positional, keywords),
            _ => {}
        }
        let signature = self
            .vocabulary
            .signature(name)
            .ok_or_else(|| ProdromeError::parse(format!("unknown constructor {name:?}")))?;
        let fields = match signature {
            Signature::AnyKeywords => {
                if !positional.is_empty() {
                    return Err(ProdromeError::parse(format!(
                        "{name}(...) takes keyword fields only in this vocabulary"
                    )));
                }
                keywords
            }
            Signature::Fields(order) => bind(name, order, positional, keywords)?,
            Signature::Owned(order) => bind(name, order, positional, keywords)?,
        };
        Ok(Value::Call(Call::new(name, fields)))
    }
}

/// Positional arguments onto declared field names, keywords checked against
/// them, the result in DECLARED order — which is what makes the print
/// canonical however the text was written.
fn bind(
    name: &str,
    order: &[impl AsRef<str>],
    positional: Vec<Value>,
    keywords: Vec<(String, Value)>,
) -> Result<Vec<(String, Value)>, ProdromeError> {
    if positional.len() > order.len() {
        return Err(ProdromeError::parse(format!(
            "bad call to {name}(...): takes at most {} arguments, got {}",
            order.len(),
            positional.len()
        )));
    }
    let taken = positional.len();
    let mut bound: Vec<(String, Value)> = order
        .iter()
        .take(taken)
        .map(|field| field.as_ref().to_owned())
        .zip(positional)
        .collect();
    for (key, value) in keywords {
        let index = order
            .iter()
            .position(|field| field.as_ref() == key)
            .ok_or_else(|| {
                ProdromeError::parse(format!("bad call to {name}(...): unexpected field {key:?}"))
            })?;
        // A keyword that names a field a positional argument already filled is
        // CPython's "got multiple values for argument".
        if index < taken {
            return Err(ProdromeError::parse(format!(
                "bad call to {name}(...): field {key:?} given twice"
            )));
        }
        bound.push((key, value));
    }
    let mut ordered: Vec<(String, Value)> = Vec::with_capacity(bound.len());
    for field in order {
        if let Some(index) = bound.iter().position(|(key, _)| key == field.as_ref()) {
            ordered.push(bound.remove(index));
        }
    }
    Ok(ordered)
}

fn integer_argument(name: &str, field: &str, value: &Value) -> Result<i64, ProdromeError> {
    match value {
        Value::Int(int) => int.as_i64().ok_or_else(|| {
            ProdromeError::parse(format!("bad call to {name}(...): {field} is out of range"))
        }),
        other => Err(ProdromeError::parse(format!(
            "bad call to {name}(...): {field} must be an integer, got {other:?}"
        ))),
    }
}

fn build_datetime(
    positional: Vec<Value>,
    keywords: Vec<(String, Value)>,
) -> Result<Value, ProdromeError> {
    const ORDER: &[&str] = &[
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "microsecond",
    ];
    let fields = bind("datetime", ORDER, positional, keywords)?;
    let mut parts = [0i64; 7];
    for (key, value) in &fields {
        let index = ORDER
            .iter()
            .position(|field| field == key)
            .expect("bind checked the name");
        parts[index] = integer_argument("datetime", key, value)?;
    }
    if fields.len() < 3 {
        return Err(ProdromeError::parse(
            "bad call to datetime(...): year, month and day are required",
        ));
    }
    let field = |index: usize| -> Result<u32, ProdromeError> {
        u32::try_from(parts[index]).map_err(|_| {
            ProdromeError::parse(format!(
                "bad call to datetime(...): {} is out of range",
                ORDER[index]
            ))
        })
    };
    let year = i32::try_from(parts[0])
        .map_err(|_| ProdromeError::parse("bad call to datetime(...): year is out of range"))?;
    Ok(Value::Datetime(Datetime::new(
        year,
        field(1)?,
        field(2)?,
        field(3)?,
        field(4)?,
        field(5)?,
        field(6)?,
    )?))
}

fn build_timedelta(
    positional: Vec<Value>,
    keywords: Vec<(String, Value)>,
) -> Result<Value, ProdromeError> {
    const ORDER: &[&str] = &[
        "days",
        "seconds",
        "microseconds",
        "milliseconds",
        "minutes",
        "hours",
        "weeks",
    ];
    let fields = bind("timedelta", ORDER, positional, keywords)?;
    let mut parts = [0i64; 7];
    for (key, value) in &fields {
        let index = ORDER
            .iter()
            .position(|field| field == key)
            .expect("bind checked the name");
        parts[index] = integer_argument("timedelta", key, value)?;
    }
    Ok(Value::Timedelta(Timedelta::new(
        parts[0], parts[1], parts[2], parts[3], parts[4], parts[5], parts[6],
    )?))
}
