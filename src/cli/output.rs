//! CLI Output Formatting.
//!
//! Handles output formatting for different formats (text, JSON, table).

use serde::Serialize;
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════════
// OUTPUT FORMAT
// ═══════════════════════════════════════════════════════════════════════════════

/// Output format options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Human-readable text
    #[default]
    Text,
    /// JSON format
    Json,
    /// Pretty JSON format
    JsonPretty,
    /// Table format
    Table,
    /// Minimal format (values only)
    Minimal,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" | "txt" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            "json-pretty" | "jsonpretty" => Ok(OutputFormat::JsonPretty),
            "table" | "tbl" => Ok(OutputFormat::Table),
            "minimal" | "min" => Ok(OutputFormat::Minimal),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OUTPUT FORMATTER
// ═══════════════════════════════════════════════════════════════════════════════

/// Output formatter for CLI
#[derive(Debug, Clone, Default)]
pub struct OutputFormatter {
    /// Output format
    format: OutputFormat,
    /// Color enabled
    color: bool,
}

impl OutputFormatter {
    /// Create new formatter
    pub fn new(format: OutputFormat) -> Self {
        Self {
            format,
            color: true,
        }
    }

    /// Disable color
    pub fn without_color(mut self) -> Self {
        self.color = false;
        self
    }

    /// Get format
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Print success message
    pub fn success(&self, message: &str) {
        match self.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                let json = serde_json::json!({
                    "status": "success",
                    "message": message
                });
                self.print_json(&json);
            }
            _ => {
                if self.color {
                    println!("\x1b[32m✓\x1b[0m {}", message);
                } else {
                    println!("OK: {}", message);
                }
            }
        }
    }

    /// Print error message
    pub fn error(&self, message: &str) {
        match self.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                let json = serde_json::json!({
                    "status": "error",
                    "message": message
                });
                self.print_json(&json);
            }
            _ => {
                if self.color {
                    eprintln!("\x1b[31m✗\x1b[0m {}", message);
                } else {
                    eprintln!("ERROR: {}", message);
                }
            }
        }
    }

    /// Print warning message
    pub fn warning(&self, message: &str) {
        match self.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                let json = serde_json::json!({
                    "status": "warning",
                    "message": message
                });
                self.print_json(&json);
            }
            _ => {
                if self.color {
                    println!("\x1b[33m⚠\x1b[0m {}", message);
                } else {
                    println!("WARNING: {}", message);
                }
            }
        }
    }

    /// Print info message
    pub fn info(&self, message: &str) {
        match self.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                let json = serde_json::json!({
                    "status": "info",
                    "message": message
                });
                self.print_json(&json);
            }
            _ => {
                if self.color {
                    println!("\x1b[34mℹ\x1b[0m {}", message);
                } else {
                    println!("INFO: {}", message);
                }
            }
        }
    }

    /// Print data
    pub fn data<T: Serialize>(&self, data: &T) {
        match self.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                self.print_json(data);
            }
            OutputFormat::Minimal => {
                if let Ok(json) = serde_json::to_value(data) {
                    self.print_minimal(&json);
                }
            }
            _ => {
                if let Ok(json) = serde_json::to_value(data) {
                    self.print_text(&json, 0);
                }
            }
        }
    }

    /// Print table
    pub fn table(&self, headers: &[&str], rows: &[Vec<String>]) {
        match self.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                let data: Vec<HashMap<&str, &str>> = rows
                    .iter()
                    .map(|row| {
                        headers
                            .iter()
                            .zip(row.iter())
                            .map(|(h, v)| (*h, v.as_str()))
                            .collect()
                    })
                    .collect();
                self.print_json(&data);
            }
            _ => {
                self.print_table_text(headers, rows);
            }
        }
    }

    /// Print key-value pair
    pub fn kv(&self, key: &str, value: &str) {
        match self.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                let json = serde_json::json!({ key: value });
                self.print_json(&json);
            }
            OutputFormat::Minimal => {
                println!("{}", value);
            }
            _ => {
                if self.color {
                    println!("\x1b[1m{}\x1b[0m: {}", key, value);
                } else {
                    println!("{}: {}", key, value);
                }
            }
        }
    }

    /// Print section header
    pub fn section(&self, title: &str) {
        if matches!(self.format, OutputFormat::Text | OutputFormat::Table) {
            println!();
            if self.color {
                println!("\x1b[1;36m=== {} ===\x1b[0m", title);
            } else {
                println!("=== {} ===", title);
            }
            println!();
        }
    }

    /// Print JSON data
    fn print_json<T: Serialize>(&self, data: &T) {
        let output = if matches!(self.format, OutputFormat::JsonPretty) {
            serde_json::to_string_pretty(data)
        } else {
            serde_json::to_string(data)
        };

        if let Ok(json) = output {
            println!("{}", json);
        }
    }

    /// Print text formatted data
    fn print_text(&self, json: &serde_json::Value, indent: usize) {
        let prefix = "  ".repeat(indent);

        match json {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    match value {
                        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                            if self.color {
                                println!("{}\x1b[1m{}\x1b[0m:", prefix, key);
                            } else {
                                println!("{}{}:", prefix, key);
                            }
                            self.print_text(value, indent + 1);
                        }
                        _ => {
                            if self.color {
                                println!("{}\x1b[1m{}\x1b[0m: {}", prefix, key, format_value(value));
                            } else {
                                println!("{}{}: {}", prefix, key, format_value(value));
                            }
                        }
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, item) in arr.iter().enumerate() {
                    println!("{}[{}]:", prefix, i);
                    self.print_text(item, indent + 1);
                }
            }
            _ => {
                println!("{}{}", prefix, format_value(json));
            }
        }
    }

    /// Print minimal output
    fn print_minimal(&self, json: &serde_json::Value) {
        match json {
            serde_json::Value::Object(map) => {
                for value in map.values() {
                    self.print_minimal(value);
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    self.print_minimal(item);
                }
            }
            _ => {
                println!("{}", format_value(json));
            }
        }
    }

    /// Print text table
    fn print_table_text(&self, headers: &[&str], rows: &[Vec<String>]) {
        if headers.is_empty() {
            return;
        }

        // Calculate column widths
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Print header
        let header_line: Vec<String> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", h, width = widths[i]))
            .collect();

        if self.color {
            println!("\x1b[1m{}\x1b[0m", header_line.join(" | "));
        } else {
            println!("{}", header_line.join(" | "));
        }

        // Print separator
        let separator: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        println!("{}", separator.join("-+-"));

        // Print rows
        for row in rows {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let width = widths.get(i).copied().unwrap_or(cell.len());
                    format!("{:width$}", cell, width = width)
                })
                .collect();
            println!("{}", cells.join(" | "));
        }
    }
}

/// Format a JSON value for text output
fn format_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRESS INDICATOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple progress indicator
#[derive(Debug)]
pub struct Progress {
    /// Total steps
    total: usize,
    /// Current step
    current: usize,
    /// Message template
    message: String,
    /// Color enabled
    color: bool,
}

impl Progress {
    /// Create new progress
    pub fn new(total: usize, message: impl Into<String>) -> Self {
        Self {
            total,
            current: 0,
            message: message.into(),
            color: true,
        }
    }

    /// Advance progress
    pub fn advance(&mut self) {
        self.current = (self.current + 1).min(self.total);
        self.render();
    }

    /// Set current value
    pub fn set(&mut self, value: usize) {
        self.current = value.min(self.total);
        self.render();
    }

    /// Complete progress
    pub fn complete(&mut self) {
        self.current = self.total;
        self.render();
        println!();
    }

    /// Render progress bar
    fn render(&self) {
        let pct = if self.total > 0 {
            (self.current * 100) / self.total
        } else {
            100
        };

        let bar_width = 30;
        let filled = (bar_width * self.current) / self.total.max(1);
        let empty = bar_width - filled;

        let bar = format!(
            "[{}{}]",
            "█".repeat(filled),
            "░".repeat(empty)
        );

        if self.color {
            print!("\r\x1b[K\x1b[1m{}\x1b[0m {} {:3}%", self.message, bar, pct);
        } else {
            print!("\r{} {} {:3}%", self.message, bar, pct);
        }

        // Flush stdout
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SPINNER
// ═══════════════════════════════════════════════════════════════════════════════

/// Loading spinner frames
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Loading spinner
#[derive(Debug)]
pub struct Spinner {
    /// Current frame
    frame: usize,
    /// Message
    message: String,
    /// Color enabled
    color: bool,
}

impl Spinner {
    /// Create new spinner
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            frame: 0,
            message: message.into(),
            color: true,
        }
    }

    /// Tick the spinner
    pub fn tick(&mut self) {
        self.frame = (self.frame + 1) % SPINNER_FRAMES.len();
        self.render();
    }

    /// Complete with success
    pub fn success(&self, message: &str) {
        if self.color {
            println!("\r\x1b[K\x1b[32m✓\x1b[0m {}", message);
        } else {
            println!("\rOK: {}", message);
        }
    }

    /// Complete with error
    pub fn error(&self, message: &str) {
        if self.color {
            eprintln!("\r\x1b[K\x1b[31m✗\x1b[0m {}", message);
        } else {
            eprintln!("\rERROR: {}", message);
        }
    }

    /// Render spinner
    fn render(&self) {
        let spinner = SPINNER_FRAMES[self.frame];

        if self.color {
            print!("\r\x1b[K\x1b[36m{}\x1b[0m {}", spinner, self.message);
        } else {
            print!("\r{} {}", spinner, self.message);
        }

        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_parse() {
        assert_eq!("text".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert!("invalid".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_formatter_creation() {
        let formatter = OutputFormatter::new(OutputFormat::Json);
        assert_eq!(formatter.format(), OutputFormat::Json);
    }

    #[test]
    fn test_progress() {
        let mut progress = Progress::new(10, "Testing");
        assert_eq!(progress.current, 0);

        progress.advance();
        assert_eq!(progress.current, 1);

        progress.set(5);
        assert_eq!(progress.current, 5);
    }

    #[test]
    fn test_spinner() {
        let mut spinner = Spinner::new("Loading");
        spinner.tick();
        assert_eq!(spinner.frame, 1);
    }

    #[test]
    fn test_format_value() {
        assert_eq!(format_value(&serde_json::Value::Null), "null");
        assert_eq!(format_value(&serde_json::json!(true)), "true");
        assert_eq!(format_value(&serde_json::json!(42)), "42");
        assert_eq!(format_value(&serde_json::json!("hello")), "hello");
    }
}
