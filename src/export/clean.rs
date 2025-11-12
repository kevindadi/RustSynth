use super::model::{DocExample, SpecDocSections};

#[derive(Debug, Clone, Default)]
pub struct CleanedDocs {
    pub summary: String,
    pub details: String,
    pub sections: SpecDocSections,
}

pub fn clean_documentation(raw: &str) -> CleanedDocs {
    if raw.trim().is_empty() {
        return CleanedDocs::default();
    }

    let mut state = DocState::default();
    for line in raw.lines() {
        state.process_line(line);
    }
    state.finish()
}

#[derive(Debug, Default)]
struct DocState {
    section: SectionKind,
    intro_parts: Vec<String>,
    details_parts: Vec<String>,
    sections: SpecDocSections,
    current_buffer: Vec<String>,
    code_buffer: Vec<String>,
    code_info: String,
    in_code_block: bool,
}

impl DocState {
    fn process_line(&mut self, line: &str) {
        let trimmed = line.trim_end();

        if is_fenced_code_start(trimmed) {
            self.flush_text();
            self.in_code_block = !self.in_code_block;
            if self.in_code_block {
                self.code_buffer.clear();
                self.code_info = trimmed.trim_start_matches('`').trim().to_string();
            } else {
                self.handle_code_block();
            }
            return;
        }

        if self.in_code_block {
            self.code_buffer.push(line.to_string());
            return;
        }

        if let Some(new_section) = parse_heading(trimmed) {
            self.flush_text();
            self.section = new_section;
            return;
        }

        let cleaned = strip_html(trimmed);
        if cleaned.is_empty() {
            self.flush_text();
        } else {
            self.current_buffer.push(cleaned);
        }
    }

    fn flush_text(&mut self) {
        if self.current_buffer.is_empty() {
            return;
        }

        let text = condense_whitespace(&self.current_buffer.join(" "));
        if text.is_empty() {
            self.current_buffer.clear();
            return;
        }

        match self.section {
            SectionKind::Intro => self.intro_parts.push(text.clone()),
            SectionKind::Errors => push_lines(&mut self.sections.errors, &text),
            SectionKind::Panics => push_lines(&mut self.sections.panics, &text),
            SectionKind::Safety => push_lines(&mut self.sections.safety, &text),
            SectionKind::Returns => push_lines(&mut self.sections.returns, &text),
            SectionKind::Examples => {
                if let Some(last) = self.sections.examples.last_mut() {
                    let notes = last.notes.get_or_insert_with(String::new);
                    if !notes.is_empty() {
                        notes.push_str("\n");
                    }
                    notes.push_str(&text);
                } else {
                    self.sections.examples.push(DocExample {
                        notes: Some(text.clone()),
                        ..DocExample::default()
                    });
                }
            }
            SectionKind::Other => {}
        }

        self.details_parts.push(text);
        self.current_buffer.clear();
    }

    fn handle_code_block(&mut self) {
        let joined = self.code_buffer.join("\n");
        if self.section == SectionKind::Examples {
            let example = classify_example(&joined, &self.code_info);
            self.sections.examples.push(example);
        } else if !joined.trim().is_empty() {
            self.details_parts.push(joined.trim().to_string());
        }
    }

    fn finish(mut self) -> CleanedDocs {
        self.flush_text();
        if self.in_code_block && !self.code_buffer.is_empty() {
            self.handle_code_block();
        }

        let summary = self
            .intro_parts
            .iter()
            .find_map(|part| {
                let text = condense_whitespace(part);
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            })
            .unwrap_or_default();

        let details = condense_whitespace(&self.details_parts.join("\n\n"));

        CleanedDocs {
            summary,
            details,
            sections: self.sections,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionKind {
    Intro,
    Errors,
    Panics,
    Safety,
    Returns,
    Examples,
    Other,
}

impl Default for SectionKind {
    fn default() -> Self {
        SectionKind::Intro
    }
}

fn parse_heading(line: &str) -> Option<SectionKind> {
    let stripped = line.trim_start_matches('#').trim();
    if stripped.is_empty() || line.chars().next() != Some('#') {
        return None;
    }

    let normalized = stripped.trim_matches(':').to_lowercase();
    let section = match normalized.as_str() {
        "errors" | "error" | "failures" => SectionKind::Errors,
        "panics" | "panic" => SectionKind::Panics,
        "safety" => SectionKind::Safety,
        "returns" | "return" => SectionKind::Returns,
        "examples" | "example" => SectionKind::Examples,
        _ => SectionKind::Other,
    };
    Some(section)
}

fn is_fenced_code_start(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("```")
}

fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            if let Some(next) = chars.peek() {
                if next.is_ascii_alphabetic() || *next == '/' || *next == '!' {
                    // skip until closing '>'
                    while let Some(next_ch) = chars.next() {
                        if next_ch == '>' {
                            break;
                        }
                    }
                    continue;
                }
            }
            out.push('<');
            continue;
        }

        out.push(ch);
    }
    out.trim().to_string()
}

fn push_lines(target: &mut Vec<String>, text: &str) {
    for line in text.split('\n') {
        let trimmed = line
            .trim()
            .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+'))
            .trim_start_matches(char::is_whitespace)
            .trim();
        if trimmed.is_empty() {
            continue;
        }
        target.push(condense_whitespace(trimmed));
    }
}

fn classify_example(code: &str, info: &str) -> DocExample {
    let trimmed = code.trim();
    let info_lower = info.to_lowercase();
    let code_lower = trimmed.to_lowercase();

    let mut example = DocExample::default();
    if info_lower.contains("should_panic") || code_lower.contains("panic!") {
        example.err = Some(trimmed.to_string());
    } else if info_lower.contains("no_run") {
        example.ok = Some(trimmed.to_string());
        example.notes = Some("non-executable example (no_run)".into());
    } else if code_lower.contains("// ok") {
        example.ok = Some(filter_comment_lines(trimmed));
    } else if code_lower.contains("// err") || code_lower.contains("// fail") {
        example.err = Some(filter_comment_lines(trimmed));
    } else {
        example.ok = Some(trimmed.to_string());
        example.notes = Some("example without explicit outcome label".into());
    }
    example
}

fn filter_comment_lines(code: &str) -> String {
    code.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn condense_whitespace(text: &str) -> String {
    let mut result = String::new();
    let mut last_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_space {
                result.push(' ');
                last_space = true;
            }
        } else {
            result.push(ch);
            last_space = false;
        }
    }
    result.trim().to_string()
}

