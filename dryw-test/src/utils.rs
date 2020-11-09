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

use std::fs;
use std::io::{self, Write};

use crossterm::cursor::MoveToColumn;
use crossterm::queue;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};

pub fn get_paths(root: &str, suffix: Option<&str>) -> Result<Vec<String>, ()> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| ())? {
        let path = entry.map_err(|_| ())?.path();
        let path_as_str = path.as_path().to_str().ok_or(())?;
        if path.is_dir() {
            paths.extend_from_slice(&get_paths(path_as_str, suffix)?);
        } else if suffix.is_none() {
            paths.push(path_as_str.to_owned());
        } else if let Some(suffix) = suffix {
            if path_as_str.ends_with(suffix) {
                paths.push(path_as_str.to_owned());
            }
        }
    }
    Ok(paths)
}

pub fn print_stats(
    s: &mut io::Stdout,
    num_passed: usize,
    num_skipped: usize,
    num_failed: usize,
) {
    queue!(
        s,
        ResetColor,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        Print("Passed: "),
        SetForegroundColor(Color::DarkGreen),
        Print(format!("{}", num_passed)),
        ResetColor,
        Print(" Failed: "),
        SetForegroundColor(Color::DarkRed),
        Print(format!("{}", num_failed)),
        ResetColor,
        Print(" Skipped: "),
        SetForegroundColor(Color::DarkYellow),
        Print(format!("{}", num_skipped)),
        ResetColor,
    )
    .unwrap();
    s.flush().unwrap();
}
