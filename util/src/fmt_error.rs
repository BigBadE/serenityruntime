use std::error::Error;
use std::fmt;
use std::ops::Deref;

use crate::error::{JsError, JsStackFrame};

const SOURCE_ABBREV_THRESHOLD: usize = 150;

// Keep in sync with `/util/error.js`.
pub fn format_location(frame: &JsStackFrame) -> String {
    if frame.is_native {
        return "<cyan>native</cyan>".to_string();
    }
    let mut result = String::new();
    if let Some(file_name) = &frame.file_name {
        result += &format!("<cyan>{}</cyan>", &file_name);
    } else {
        if frame.is_eval {
            result +=
                &format!("<cyan>{}</cyan>, ", &frame.eval_origin.as_ref().unwrap());
        }
        result += &"<cyan><anonymous></cyan>".to_string();
    }
    if let Some(line_number) = frame.line_number {
        result += &format!(":<yellow>{}</yellow>", &line_number);
        if let Some(column_number) = frame.column_number {
            result += &format!(":<yellow>{}</yellow>", &column_number);
        }
    }
    result
}

fn format_frame(frame: &JsStackFrame) -> String {
    let is_method_call =
        !(frame.is_top_level.unwrap_or_default() || frame.is_constructor);
    let mut result = String::new();
    if frame.is_async {
        result += "async ";
    }
    if frame.is_promise_all {
        result += &format!(
            "<italic><bold>Promise.all (index {})</bold></italic>",
            frame.promise_index.unwrap_or_default()
        )
            .to_string();
        return result;
    }
    if is_method_call {
        let mut formatted_method = String::new();
        if let Some(function_name) = &frame.function_name {
            if let Some(type_name) = &frame.type_name {
                if !function_name.starts_with(type_name) {
                    formatted_method += &format!("{}.", type_name);
                }
            }
            formatted_method += function_name;
            if let Some(method_name) = &frame.method_name {
                if !function_name.ends_with(method_name) {
                    formatted_method += &format!(" [as {}]", method_name);
                }
            }
        } else {
            if let Some(type_name) = &frame.type_name {
                formatted_method += &format!("{}.", type_name);
            }
            if let Some(method_name) = &frame.method_name {
                formatted_method += method_name
            } else {
                formatted_method += "<anonymous>";
            }
        }
        result += &format!("<italic><bold>{}</bold></italic>", &formatted_method);
    } else if frame.is_constructor {
        result += "new ";
        if let Some(function_name) = &frame.function_name {
            result += &format!("<italic><bold>{}</bold></italic>", &function_name);
        } else {
            result += &"<cyan><anonymous></cyan>".to_string();
        }
    } else if let Some(function_name) = &frame.function_name {
        result += &format!("<italic><bold>{}</bold></italic>", &function_name).to_string();
    } else {
        result += &format_location(frame);
        return result;
    }
    result += &format!(" ({})", format_location(frame));
    result
}

#[allow(clippy::too_many_arguments)]
fn format_stack(
    is_error: bool,
    message_line: &str,
    cause: Option<&str>,
    source_line: Option<&str>,
    start_column: Option<i64>,
    end_column: Option<i64>,
    frames: &[JsStackFrame],
    level: usize,
) -> String {
    let mut s = String::new();
    s.push_str(&format!("{:indent$}{}", "", message_line, indent = level));
    s.push_str(&format_maybe_source_line(
        source_line,
        start_column,
        end_column,
        is_error,
        level,
    ));
    for frame in frames {
        s.push_str(&format!(
            "\n{:indent$}    at {}",
            "",
            format_frame(frame),
            indent = level
        ));
    }
    if let Some(cause) = cause {
        s.push_str(&format!(
            "\n{:indent$}Caused by: {}",
            "",
            cause,
            indent = level
        ));
    }
    s
}

/// Take an optional source line and associated information to format it into
/// a pretty printed version of that line.
fn format_maybe_source_line(
    source_line: Option<&str>,
    start_column: Option<i64>,
    end_column: Option<i64>,
    is_error: bool,
    level: usize,
) -> String {
    if source_line.is_none() || start_column.is_none() || end_column.is_none() {
        return "".to_string();
    }

    let source_line = source_line.unwrap();
    // sometimes source_line gets set with an empty string, which then outputs
    // an empty source line when displayed, so need just short circuit here.
    // Also short-circuit on error line too long.
    if source_line.is_empty() || source_line.len() > SOURCE_ABBREV_THRESHOLD {
        return "".to_string();
    }
    if source_line.contains("Couldn't format source line: ") {
        return format!("\n{}", source_line);
    }

    assert!(start_column.is_some());
    assert!(end_column.is_some());
    let mut s = String::new();
    let start_column = start_column.unwrap();
    let end_column = end_column.unwrap();

    if start_column as usize >= source_line.len() {
        return format!(
            "\n<yellow>Warning</yellow> Couldn't format source line: Column {} is out of bounds (source may have changed at runtime)",
            start_column + 1,
        );
    }

    for _i in 0..start_column {
        if source_line.chars().nth(_i as usize).unwrap() == '\t' {
            s.push('\t');
        } else {
            s.push(' ');
        }
    }
    for _i in 0..(end_column - start_column) {
        s.push('^');
    }
    let color_underline = if is_error {
        format!("<red>{}</red>", &s)
    } else {
        format!("<cyan>{}</cyan>", &s)
    };

    let indent = format!("{:indent$}", "", indent = level);

    format!("\n{}{}\n{}{}", indent, source_line, indent, color_underline)
}

/// Wrapper around deno_core::JsError which provides colorful
/// string representation.
#[derive(Debug)]
pub struct PrettyJsError(JsError);

impl PrettyJsError {
    pub fn create(js_error: JsError) -> anyhow::Error {
        let pretty_js_error = Self(js_error);
        pretty_js_error.into()
    }
}

impl Deref for PrettyJsError {
    type Target = JsError;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for PrettyJsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut frames = self.0.frames.clone();

        // When the stack frame array is empty, but the source location given by
        // (script_resource_name, line_number, start_column + 1) exists, this is
        // likely a syntax error. For the sake of formatting we treat it like it was
        // given as a single stack frame.
        if frames.is_empty()
            && self.0.script_resource_name.is_some()
            && self.0.line_number.is_some()
            && self.0.start_column.is_some()
        {
            frames = vec![JsStackFrame::from_location(
                self.0.script_resource_name.clone(),
                self.0.line_number,
                self.0.start_column.map(|n| n + 1),
            )];
        }

        let cause = self
            .0
            .cause
            .clone()
            .map(|cause| format!("{}", PrettyJsError(*cause)));

        write!(
            f,
            "{}",
            &format_stack(
                true,
                &self.0.message,
                cause.as_deref(),
                self.0.source_line.as_deref(),
                self.0.start_column,
                self.0.end_column,
                &frames,
                0
            )
        )?;
        Ok(())
    }
}

impl Error for PrettyJsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_none_source_line() {
        let actual = format_maybe_source_line(None, None, None, false, 0);
        assert_eq!(actual, "");
    }

    #[test]
    fn test_format_some_source_line() {
        let actual = format_maybe_source_line(
            Some("console.log('foo');"),
            Some(8),
            Some(11),
            true,
            0,
        );
        assert_eq!(
            &actual,
            "\nconsole.log(\'foo\');\n<red>        ^^^</red>"
        );
    }
}