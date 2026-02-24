use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSection {
    Lexer,
    Parser,
    Checker,
    Codegen,
}

impl DiagnosticSection {
    pub fn base_code(self) -> u32 {
        match self {
            DiagnosticSection::Lexer => 100,
            DiagnosticSection::Parser => 200,
            DiagnosticSection::Checker => 400,
            DiagnosticSection::Codegen => 500,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticInfo {
    pub code: u32,
    pub severity: Severity,
    pub section: DiagnosticSection,
}

#[derive(Debug, Clone)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

impl Label {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    pub info: DiagnosticInfo,
    pub message: String,
    pub labels: Vec<Label>,
}

pub trait Diagnostic {
    fn get_diagnostic_info(&self) -> DiagnosticInfo;
    fn get_diagnostic_message(&self) -> String;
    fn get_diagnostic_labels(&self) -> Vec<Label>;

    fn to_report(&self) -> DiagnosticReport {
        DiagnosticReport {
            info: self.get_diagnostic_info(),
            message: self.get_diagnostic_message(),
            labels: self.get_diagnostic_labels(),
        }
    }
}

#[derive(Debug, Default)]
pub struct DiagnosticBag {
    reports: Vec<DiagnosticReport>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
        }
    }

    pub fn add(&mut self, diag: &dyn Diagnostic) {
        self.reports.push(diag.to_report());
    }

    pub fn add_report(&mut self, report: DiagnosticReport) {
        self.reports.push(report);
    }

    pub fn has_errors(&self) -> bool {
        self.reports.iter().any(|r| r.info.severity == Severity::Error)
    }

    pub fn reports(&self) -> &[DiagnosticReport] {
        &self.reports
    }

    pub fn merge(&mut self, other: DiagnosticBag) {
        self.reports.extend(other.reports);
    }

    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }
}

#[derive(Debug)]
pub struct DiagnosticError {
    pub reports: Vec<DiagnosticReport>,
}

impl std::fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for r in &self.reports {
            writeln!(f, "[E{:04}] {}", r.info.code, r.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for DiagnosticError {}

impl From<DiagnosticBag> for DiagnosticError {
    fn from(bag: DiagnosticBag) -> Self {
        Self {
            reports: bag.reports,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_bag_has_errors() {
        let mut bag = DiagnosticBag::new();
        assert!(!bag.has_errors());

        bag.add_report(DiagnosticReport {
            info: DiagnosticInfo {
                code: 100,
                severity: Severity::Warning,
                section: DiagnosticSection::Lexer,
            },
            message: "test warning".into(),
            labels: vec![],
        });
        assert!(!bag.has_errors());

        bag.add_report(DiagnosticReport {
            info: DiagnosticInfo {
                code: 101,
                severity: Severity::Error,
                section: DiagnosticSection::Lexer,
            },
            message: "test error".into(),
            labels: vec![],
        });
        assert!(bag.has_errors());
    }
}
