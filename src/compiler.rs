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

use crate::scanner;

pub fn compile(source: String) {
    let mut scanner = scanner::Scanner::from_source(source);
    let mut line: usize = 0;
    loop {
        let token: scanner::Token = scanner.scan_token();
        if token.line != line {
            print!("{:4} ", token.line);
            line = token.line;
        } else {
            print!("   | ");
        }
        println!("{:2} '{}'", token.kind as i32, token.source);

        match token.kind {
            scanner::TokenKind::Eof => {
                break;
            }
            _ => {}
        }
    }
}
