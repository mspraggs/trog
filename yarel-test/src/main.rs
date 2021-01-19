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

use regex::{self, Regex};

const WILDCARDS: [(&str, &str); 1] = [("[MEMADDR]", "0x[a-f0-9]+")];

fn parse_line(source: &str) -> String {
    let mut result = regex::escape(source);
    for (wildcard, regex) in &WILDCARDS {
        let escaped_wildcard = &regex::escape(wildcard.to_owned());
        result = result.replace(escaped_wildcard, regex);
    }

    format!("^{}$", result)
}

fn match_output(expected: &[String], actual: &[String]) -> bool {
    if expected.len() != actual.len() {
        return false;
    }

    if expected == actual {
        return true;
    }

    for (expected, actual) in expected.iter().zip(actual.iter()) {
        if expected == actual {
            continue;
        }
        let expected = parse_line(expected);
        let re = Regex::new(&expected).unwrap();
        if !re.is_match(actual) {
            return false;
        }
    }

    true
}

fn main() {
    let expected_output = vec!["[MEMADDR]".to_owned()];
    let output = vec!["0x1234".to_owned()];

    println!("{}", !match_output(&expected_output, &output));
}
