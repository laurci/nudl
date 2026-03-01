use tower_lsp::lsp_types::*;

use nudl_core::diagnostic::{DiagnosticBag, DiagnosticReport, Severity};
use nudl_core::source::SourceMap;
use nudl_core::span::FileId;

pub fn convert_diagnostics(
    bag: &DiagnosticBag,
    source_map: &SourceMap,
    file_id: FileId,
) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    for report in bag.reports() {
        // Only show diagnostics from the current file
        let has_label_in_file = report
            .labels
            .iter()
            .any(|label| label.span.file_id == file_id);
        if !has_label_in_file && !report.labels.is_empty() {
            continue;
        }

        let severity = match report.info.severity {
            Severity::Error => Some(DiagnosticSeverity::ERROR),
            Severity::Warning => Some(DiagnosticSeverity::WARNING),
            Severity::Info => Some(DiagnosticSeverity::INFORMATION),
        };

        let range = report_range(report, source_map, file_id);

        result.push(Diagnostic {
            range,
            severity,
            code: Some(NumberOrString::Number(report.info.code as i32)),
            source: Some("nudl".into()),
            message: report.message.clone(),
            ..Default::default()
        });
    }

    result
}

pub fn report_range(report: &DiagnosticReport, source_map: &SourceMap, file_id: FileId) -> Range {
    if let Some(label) = report.labels.first() {
        let span = label.span;
        if span.file_id == file_id {
            let file = source_map.get_file(span.file_id);

            if span.is_empty() {
                let offset = span.start.min(file.content.len().saturating_sub(1) as u32);
                let (line, col) = file.line_col(offset);
                let pos = Position::new(line - 1, col - 1);
                return Range {
                    start: pos,
                    end: pos,
                };
            }

            let (start_line, start_col) = file.line_col(span.start);
            let (end_line, end_col) = file.line_col(span.end.min(file.content.len() as u32));
            return Range {
                start: Position::new(start_line - 1, start_col - 1),
                end: Position::new(end_line - 1, end_col - 1),
            };
        }
    }

    Range {
        start: Position::new(0, 0),
        end: Position::new(0, 0),
    }
}
