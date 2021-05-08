/* Copyright 2020-2021 Matt Spraggs
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::common;

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum TokenKind {
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Comma,
    Dot,
    DotDot,
    Minus,
    MinusEqual,
    Plus,
    PlusEqual,
    Colon,
    SemiColon,
    Slash,
    SlashEqual,
    Star,
    StarEqual,
    Bang,
    BangEqual,
    Equal,
    EqualEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Amp,
    AmpEqual,
    Bar,
    BarEqual,
    Caret,
    CaretEqual,
    Percent,
    PercentEqual,
    GreaterGreater,
    GreaterGreaterEqual,
    LessLess,
    LessLessEqual,
    AmpAmp,
    BarBar,
    Tilde,
    Hash,
    Identifier,
    Str,
    Interpolation,
    Number,
    CapSelf,
    Catch,
    Class,
    Else,
    False,
    Finally,
    For,
    Fn,
    If,
    Import,
    As,
    In,
    Nil,
    Return,
    Self_,
    Super,
    Throw,
    True,
    Try,
    Var,
    While,
    Error,
    Eof,
}

impl Default for TokenKind {
    fn default() -> Self {
        TokenKind::Eof
    }
}

#[derive(Default, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub source: String,
}

impl Token {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn from_string(source: &str) -> Self {
        Token {
            kind: Default::default(),
            line: Default::default(),
            source: String::from(source),
        }
    }

    pub fn from_string_and_line(source: &str, line: usize) -> Self {
        Token {
            kind: Default::default(),
            line,
            source: String::from(source),
        }
    }
}

fn is_alpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic() || c == '_')
}

fn is_digit(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

pub struct Scanner {
    source: String,
    start: usize,
    current: usize,
    line: usize,
    parantheses: Vec<usize>,
}

impl Scanner {
    pub fn from_source(source: String) -> Self {
        Scanner {
            source,
            start: 0,
            current: 0,
            line: 1,
            parantheses: Vec::new(),
        }
    }

    pub fn scan_token(&mut self) -> Token {
        self.skip_whitespace();

        self.start = self.current;

        if self.is_at_end() {
            return self.make_token(TokenKind::Eof);
        }

        let c = self.advance();

        if is_alpha(c) {
            return self.identifier();
        }
        if is_digit(c) {
            return self.number();
        }

        match c {
            "(" => self.make_token(TokenKind::LeftParen),
            ")" => self.make_token(TokenKind::RightParen),
            "{" => {
                if let Some(count) = self.parantheses.last_mut() {
                    *count += 1;
                }
                self.make_token(TokenKind::LeftBrace)
            }
            "}" => {
                if let Some(count) = self.parantheses.last_mut() {
                    *count -= 1;
                    if *count == 0 {
                        self.parantheses.pop();
                        self.string()
                    } else {
                        self.make_token(TokenKind::RightBrace)
                    }
                } else {
                    self.make_token(TokenKind::RightBrace)
                }
            }
            "[" => self.make_token(TokenKind::LeftBracket),
            "]" => self.make_token(TokenKind::RightBracket),
            ":" => self.make_token(TokenKind::Colon),
            ";" => self.make_token(TokenKind::SemiColon),
            "," => self.make_token(TokenKind::Comma),
            "#" => self.make_token(TokenKind::Hash),
            "." => {
                let match_char = self.match_char(".");
                self.make_token(if match_char {
                    TokenKind::DotDot
                } else {
                    TokenKind::Dot
                })
            }
            "-" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::MinusEqual
                } else {
                    TokenKind::Minus
                })
            }
            "+" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::PlusEqual
                } else {
                    TokenKind::Plus
                })
            }
            "/" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::SlashEqual
                } else {
                    TokenKind::Slash
                })
            }
            "*" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::StarEqual
                } else {
                    TokenKind::Star
                })
            }
            "!" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::BangEqual
                } else {
                    TokenKind::Bang
                })
            }
            "=" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::EqualEqual
                } else {
                    TokenKind::Equal
                })
            }
            "<" => {
                let double_less = self.match_char("<");
                let equal = self.match_char("=");
                let token_kind = match (double_less, equal) {
                    (true, true) => TokenKind::LessLessEqual,
                    (true, false) => TokenKind::LessLess,
                    (false, true) => TokenKind::LessEqual,
                    (false, false) => TokenKind::Less,
                };
                self.make_token(token_kind)
            }
            ">" => {
                let double_greater = self.match_char(">");
                let equal = self.match_char("=");
                let token_kind = match (double_greater, equal) {
                    (true, true) => TokenKind::GreaterGreaterEqual,
                    (true, false) => TokenKind::GreaterGreater,
                    (false, true) => TokenKind::GreaterEqual,
                    (false, false) => TokenKind::Greater,
                };
                self.make_token(token_kind)
            }
            "|" => {
                let token_kind = if self.match_char("|") {
                    TokenKind::BarBar
                } else if self.match_char("=") {
                    TokenKind::BarEqual
                } else {
                    TokenKind::Bar
                };
                self.make_token(token_kind)
            }
            "&" => {
                let token_kind = if self.match_char("&") {
                    TokenKind::AmpAmp
                } else if self.match_char("=") {
                    TokenKind::AmpEqual
                } else {
                    TokenKind::Amp
                };
                self.make_token(token_kind)
            }
            "^" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::CaretEqual
                } else {
                    TokenKind::Caret
                })
            }
            "%" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::PercentEqual
                } else {
                    TokenKind::Percent
                })
            }
            "~" => self.make_token(TokenKind::Tilde),
            "\"" => self.string(),
            c => {
                let msg = format!("Unexpected character: '{}'.", c);
                self.error_token(msg.as_str())
            }
        }
    }

    fn is_at_end(&self) -> bool {
        self.current >= self.source.len()
    }

    fn advance(&mut self) -> &str {
        let slice_start = self.current;
        self.current = self.get_next_char_boundary(self.current);
        &self.source[slice_start..self.current]
    }

    fn peek(&self) -> &str {
        let slice_end = self.get_next_char_boundary(self.current);
        &self.source[self.current..slice_end]
    }

    fn peek_next(&self) -> &str {
        if self.is_at_end() {
            return "";
        }
        let slice_start = self.get_next_char_boundary(self.current);
        let slice_end = self.get_next_char_boundary(slice_start);
        &self.source[slice_start..slice_end]
    }

    fn match_char(&mut self, expected: &str) -> bool {
        if self.is_at_end() {
            return false;
        }
        let next = self.get_next_char_boundary(self.current);
        if &self.source[self.current..next] != expected {
            return false;
        }
        self.current = next;
        true
    }

    fn make_token(&self, kind: TokenKind) -> Token {
        Token {
            kind,
            line: self.line,
            source: String::from(&self.source[self.start..self.current]),
        }
    }

    fn error_token(&self, message: &str) -> Token {
        Token {
            kind: TokenKind::Error,
            line: self.line,
            source: String::from(message),
        }
    }

    fn skip_whitespace(&mut self) {
        loop {
            if self.is_at_end() {
                return;
            }
            let c = self.peek();
            match c {
                " " => {
                    self.advance();
                }
                "\r" => {
                    self.advance();
                }
                "\t" => {
                    self.advance();
                }
                "\n" => {
                    self.line += 1;
                    self.advance();
                }
                "/" => {
                    if self.peek_next() == "/" {
                        while !self.is_at_end() && self.peek() != "\n" {
                            self.advance();
                        }
                    } else {
                        return;
                    }
                }
                _ => {
                    return;
                }
            };
        }
    }

    fn check_keyword(&self, start: usize, rest: &str, kind: TokenKind) -> TokenKind {
        let slice_begin = self.start + start;
        let slice_end = slice_begin + rest.len();

        if self.current - self.start == start + rest.len()
            && &self.source[slice_begin..slice_end] == rest
        {
            return kind;
        }
        TokenKind::Identifier
    }

    fn identifier_type(&self) -> TokenKind {
        let start = &self.source[self.start..self.start + 1];
        match start {
            "a" => self.check_keyword(1, "s", TokenKind::As),
            "c" => {
                if self.current - self.start > 1 {
                    let next = &self.source[self.start + 1..self.start + 2];
                    return match next {
                        "a" => self.check_keyword(2, "tch", TokenKind::Catch),
                        "l" => self.check_keyword(2, "ass", TokenKind::Class),
                        _ => TokenKind::Identifier,
                    };
                }
                TokenKind::Identifier
            }
            "e" => self.check_keyword(1, "lse", TokenKind::Else),
            "f" => {
                if self.current - self.start > 1 {
                    let next = &self.source[self.start + 1..self.start + 2];
                    return match next {
                        "a" => self.check_keyword(2, "lse", TokenKind::False),
                        "i" => self.check_keyword(2, "nally", TokenKind::Finally),
                        "o" => self.check_keyword(2, "r", TokenKind::For),
                        "n" => self.check_keyword(2, "", TokenKind::Fn),
                        _ => TokenKind::Identifier,
                    };
                }
                TokenKind::Identifier
            }
            "i" => {
                if self.current - self.start > 1 {
                    let next = &self.source[self.start + 1..self.start + 2];
                    return match next {
                        "f" => self.check_keyword(2, "", TokenKind::If),
                        "n" => self.check_keyword(2, "", TokenKind::In),
                        "m" => self.check_keyword(2, "port", TokenKind::Import),
                        _ => TokenKind::Identifier,
                    };
                }
                TokenKind::Identifier
            }
            "n" => self.check_keyword(1, "il", TokenKind::Nil),
            "r" => self.check_keyword(1, "eturn", TokenKind::Return),
            "S" => self.check_keyword(1, "elf", TokenKind::CapSelf),
            "s" => {
                if self.current - self.start > 1 {
                    let next = &self.source[self.start + 1..self.start + 2];
                    return match next {
                        "e" => self.check_keyword(2, "lf", TokenKind::Self_),
                        "u" => self.check_keyword(2, "per", TokenKind::Super),
                        _ => TokenKind::Identifier,
                    };
                }
                TokenKind::Identifier
            }
            "t" => {
                if self.current - self.start > 1 {
                    let next = &self.source[self.start + 1..self.start + 2];
                    return match next {
                        "h" => self.check_keyword(2, "row", TokenKind::Throw),
                        "r" => {
                            if self.current - self.start > 2 {
                                let next = &self.source[self.start + 2..self.start + 3];
                                match next {
                                    "u" => self.check_keyword(3, "e", TokenKind::True),
                                    "y" => self.check_keyword(3, "", TokenKind::Try),
                                    _ => TokenKind::Identifier,
                                }
                            } else {
                                TokenKind::Identifier
                            }
                        }
                        _ => TokenKind::Identifier,
                    };
                }
                TokenKind::Identifier
            }
            "v" => self.check_keyword(1, "ar", TokenKind::Var),
            "w" => self.check_keyword(1, "hile", TokenKind::While),
            _ => TokenKind::Identifier,
        }
    }

    fn identifier(&mut self) -> Token {
        while is_alpha(self.peek()) || is_digit(self.peek()) {
            self.advance();
        }
        self.make_token(self.identifier_type())
    }

    fn number(&mut self) -> Token {
        while is_digit(self.peek()) {
            self.advance();
        }

        if self.peek() == "." && is_digit(self.peek_next()) {
            self.advance();

            while is_digit(self.peek()) {
                self.advance();
            }
        }

        self.make_token(TokenKind::Number)
    }

    fn read_escaped_bytes(&mut self, num_bytes: usize) -> Result<String, ()> {
        let mut bytes = Vec::with_capacity(num_bytes);
        for _ in 0..num_bytes {
            let mut read_chars = String::new();
            for _ in 0..2 {
                if self.is_at_end() {
                    return Err(());
                }
                let slice_start = self.current;
                let chars = self.advance();
                if chars == "\"" {
                    self.current = slice_start;
                    return Err(());
                }
                read_chars.push_str(chars);
            }
            let result = u8::from_str_radix(read_chars.as_str(), 16);
            match result {
                Ok(b) => bytes.push(b),
                Err(_) => {
                    return Err(());
                }
            }
        }
        if num_bytes == 1 && *bytes.last().unwrap() > 127_u8 {
            bytes.insert(0, 195);
            *bytes.last_mut().unwrap() &= 0b1011_1111;
        }
        match String::from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(_) => Err(()),
        }
    }

    fn string(&mut self) -> Token {
        let mut error = None;
        let mut buffer = String::new();

        while !self.is_at_end() && self.peek() != "\"" {
            let s = self.advance();

            match s {
                "$" => {
                    let s = self.advance();
                    if s != "{" {
                        return self.error_token("Expected '{' in string interpolation.");
                    }
                    if self.parantheses.len() >= common::INTERPOLATION_DEPTH_MAX {
                        return self.error_token("Max interpolation depth exceeded.");
                    }
                    self.parantheses.push(1);
                    return Token {
                        line: self.line,
                        source: buffer,
                        kind: TokenKind::Interpolation,
                    };
                }
                "\\" => {
                    let s = self.advance();
                    match s {
                        "$" => buffer.push_str("$"),
                        "a" => buffer.push_str("\x07"),
                        "b" => buffer.push_str("\x08"),
                        "f" => buffer.push_str("\x0c"),
                        "n" => buffer.push_str("\n"),
                        "r" => buffer.push_str("\r"),
                        "t" => buffer.push_str("\t"),
                        "u" => {
                            let result = self.read_escaped_bytes(2);
                            match result {
                                Ok(s) => buffer.push_str(s.as_str()),
                                Err(_) => {
                                    error = Some("Invalid Unicode sequence.");
                                }
                            }
                        }
                        "U" => {
                            let result = self.read_escaped_bytes(4);
                            match result {
                                Ok(s) => buffer.push_str(s.as_str()),
                                Err(_) => {
                                    error = Some("Invalid Unicode sequence.");
                                }
                            }
                        }
                        "v" => buffer.push_str("\x0b"),
                        "x" => {
                            let result = self.read_escaped_bytes(1);
                            match result {
                                Ok(s) => buffer.push_str(s.as_str()),
                                Err(_) => {
                                    error = Some("Invalid hexadecimal sequence.");
                                }
                            }
                        }
                        "\"" => buffer.push_str("\""),
                        "\\" => buffer.push_str("\\"),
                        "0" => buffer.push_str("\0"),
                        _ => {
                            return self.error_token("Invalid escape sequence.");
                        }
                    }
                }
                "\n" => {
                    buffer.push_str(s);
                    self.line += 1;
                }
                _ => buffer.push_str(s),
            }
        }

        if self.is_at_end() {
            return self.error_token("Unterminated string.");
        }
        self.advance();
        if let Some(msg) = error {
            return self.error_token(msg);
        }

        Token {
            line: self.line,
            source: buffer,
            kind: TokenKind::Str,
        }
    }

    fn get_next_char_boundary(&self, start: usize) -> usize {
        for pos in (start + 1)..self.source.len() {
            if self.source.is_char_boundary(pos) {
                return pos;
            }
        }
        self.source.len()
    }
}
