use miette::{self, MietteDiagnostic as MietteReport, NamedSource, Report, SourceSpan};
use nudl_core::diagnostic::{DiagnosticBag, DiagnosticReport, Severity};
use nudl_core::source::SourceMap;

pub fn render_diagnostics(bag: &DiagnosticBag, source_map: &SourceMap) {
    for report in bag.reports() {
        render_one(report, source_map);
    }
}

fn render_one(report: &DiagnosticReport, source_map: &SourceMap) {
    if report.labels.is_empty() {
        let severity_str = match report.info.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        eprintln!(
            "{}: [E{:04}] {}",
            severity_str, report.info.code, report.message
        );
        return;
    }

    for label in &report.labels {
        let span = label.span;
        let file = source_map.get_file(span.file_id);
        // Use miette for fancy rendering
        let source = NamedSource::new(file.path.display().to_string(), file.content.clone());

        let miette_span = SourceSpan::new(
            (span.start as usize).into(),
            (span.end - span.start) as usize,
        );

        let diag = MietteReport::new(format!("[E{:04}] {}", report.info.code, report.message))
            .with_severity(match report.info.severity {
                Severity::Error => miette::Severity::Error,
                Severity::Warning => miette::Severity::Warning,
                Severity::Info => miette::Severity::Advice,
            })
            .with_label(miette::LabeledSpan::at(miette_span, &label.message));

        let report = Report::new(diag).with_source_code(source);
        eprintln!("{:?}", report);
    }
}
