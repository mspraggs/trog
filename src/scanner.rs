/* Copyright 2020 Matt Spraggs
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

#[derive(Copy, Clone, PartialEq)]
#[repr(u32)]
pub enum TokenKind {
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Comma,
    Dot,
    Minus,
    Plus,
    SemiColon,
    Slash,
    Star,
    Bang,
    BangEqual,
    Equal,
    EqualEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Identifier,
    Str,
    Number,
    And,
    Class,
    Else,
    False,
    For,
    Fun,
    If,
    Nil,
    Or,
    Print,
    Return,
    Super,
    This,
    True,
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
}

fn is_alpha(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_alphabetic() || c == '_')
}

fn is_digit(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_digit())
}

pub struct Scanner {
    source: String,
    start: usize,
    current: usize,
    line: usize,
}

impl Scanner {
    pub fn from_source(source: String) -> Self {
        Scanner {
            source,
            start: 0,
            current: 0,
            line: 1,
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
            "{" => self.make_token(TokenKind::LeftBrace),
            "}" => self.make_token(TokenKind::RightBrace),
            ";" => self.make_token(TokenKind::SemiColon),
            "," => self.make_token(TokenKind::Comma),
            "." => self.make_token(TokenKind::Dot),
            "-" => self.make_token(TokenKind::Minus),
            "+" => self.make_token(TokenKind::Plus),
            "/" => self.make_token(TokenKind::Slash),
            "*" => self.make_token(TokenKind::Star),
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
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::LessEqual
                } else {
                    TokenKind::Less
                })
            }
            ">" => {
                let match_char = self.match_char("=");
                self.make_token(if match_char {
                    TokenKind::GreaterEqual
                } else {
                    TokenKind::Greater
                })
            }
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
        if &self.source[self.current..self.current + 1] != expected {
            return false;
        }
        self.current += 1;
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
                        while self.peek() != "\n" && !self.is_at_end() {
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
            "a" => self.check_keyword(1, "nd", TokenKind::And),
            "c" => self.check_keyword(1, "lass", TokenKind::Class),
            "e" => self.check_keyword(1, "lse", TokenKind::Else),
            "f" => {
                if self.current - self.start > 1 {
                    let next = &self.source[self.start + 1..self.start + 2];
                    return match next {
                        "a" => self.check_keyword(2, "lse", TokenKind::False),
                        "o" => self.check_keyword(2, "r", TokenKind::For),
                        "u" => self.check_keyword(2, "n", TokenKind::Fun),
                        _ => TokenKind::Identifier,
                    };
                }
                TokenKind::Identifier
            }
            "i" => self.check_keyword(1, "f", TokenKind::If),
            "n" => self.check_keyword(1, "il", TokenKind::Nil),
            "o" => self.check_keyword(1, "r", TokenKind::Or),
            "p" => self.check_keyword(1, "rint", TokenKind::Print),
            "r" => self.check_keyword(1, "eturn", TokenKind::Return),
            "s" => self.check_keyword(1, "uper", TokenKind::Super),
            "t" => {
                if self.current - self.start > 1 {
                    let next = &self.source[self.start + 1..self.start + 2];
                    return match next {
                        "h" => self.check_keyword(2, "is", TokenKind::This),
                        "r" => self.check_keyword(2, "ue", TokenKind::True),
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

    fn string(&mut self) -> Token {
        while !self.is_at_end() && self.peek() != "\"" {
            if self.peek() == "\n" {
                self.line += 1;
            }
            self.advance();
        }

        if self.is_at_end() {
            return self.error_token("Unterminated string.");
        }

        self.advance();
        self.make_token(TokenKind::Str)
    }

    fn get_next_char_boundary(&self, start: usize) -> usize {
        let mut pos = start + 1;
        while pos <= self.source.len() && !self.source.as_str().is_char_boundary(pos) {
            pos += 1;
        }
        pos
    }
}
