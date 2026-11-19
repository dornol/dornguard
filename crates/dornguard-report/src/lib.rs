use std::io::{self, Write};

use dornguard_core::{CheckReport, Scope, Severity, Status};

mod sarif;

pub use sarif::write_sarif;

pub fn write_json(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, report)?;
    writeln!(writer)
}

pub fn write_text(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    write_summary(report, writer)?;
    for diagnostic in &report.diagnostics {
        write_diagnostic(diagnostic, writer)?;
    }
    write_failures(report, writer)
}

pub fn write_agent(report: &CheckReport, writer: &mut impl Write, limit: usize) -> io::Result<()> {
    write_summary(report, writer)?;
    let errors = report
        .diagnostics
        .iter()
        .filter(|item| item.severity == Severity::Error);
    let warnings = report
        .diagnostics
        .iter()
        .filter(|item| item.severity == Severity::Warning);
    writeln!(
        writer,
        "{} errors, {} warnings, {} baselined",
        errors.clone().count(),
        warnings.clone().count(),
        report.suppressed_count
    )?;
    write_failures(report, writer)?;
    for diagnostic in errors.chain(warnings).take(limit) {
        write_diagnostic(diagnostic, writer)?;
    }
    if report.diagnostics.len() > limit {
        writeln!(
            writer,
            "{} more diagnostics. Use --format json for the complete report.",
            report.diagnostics.len() - limit
        )?;
    }
    Ok(())
}

fn write_summary(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    let status = match report.status {
        Status::Passed => "PASSED",
        Status::NoChanges => "NO CHANGES",
        Status::Violations => "FAILED",
        Status::ConfigurationError => "CONFIGURATION ERROR",
        Status::EngineError => "ENGINE ERROR",
        Status::PreparationError => "PREPARATION ERROR",
    };
    let scope = match report.scope {
        Scope::Full => "full",
        Scope::Selected => "selected",
        Scope::Changed => "changed",
    };
    writeln!(
        writer,
        "DornGuard: {status} ({scope}, {} files scanned)",
        report.files_scanned
    )?;
    if let Some(reason) = &report.fallback_reason {
        writeln!(writer, "Full scan: {reason}")?;
    }
    Ok(())
}

fn write_failures(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    for failure in &report.failures {
        writeln!(writer, "  {}", failure.message)?;
    }
    Ok(())
}

fn write_diagnostic(
    diagnostic: &dornguard_core::Diagnostic,
    writer: &mut impl Write,
) -> io::Result<()> {
    let severity = match diagnostic.severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
    };
    if let Some(path) = &diagnostic.path {
        write!(writer, "{path}")?;
    } else if let (Some(module), Some(class_name)) = (&diagnostic.module, &diagnostic.class_name) {
        write!(writer, "{module} [{class_name}]")?;
    }
    if let Some(line) = diagnostic.line {
        write!(writer, ":{line}")?;
        if let Some(column) = diagnostic.column {
            write!(writer, ":{column}")?;
        }
    }
    writeln!(
        writer,
        ": {severity}[{}]: {}",
        diagnostic.rule_id, diagnostic.message
    )?;
    writeln!(writer, "  Fix: {}", diagnostic.fix)
}
