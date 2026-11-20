use std::collections::BTreeSet;
use std::io::{self, Write};

use dornguard_core::{CheckReport, FailureStage, Scope, Severity, Status};

mod sarif;

pub use sarif::write_sarif;

pub fn write_json(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, report)?;
    writeln!(writer)
}

pub fn write_text(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    write_summary(report, writer)?;
    write_failures(report, writer)?;
    write_rule_counts(report, writer)?;
    for diagnostic in &report.diagnostics {
        write_diagnostic(diagnostic, writer)?;
    }
    Ok(())
}

pub fn write_agent(report: &CheckReport, writer: &mut impl Write, limit: usize) -> io::Result<()> {
    write_summary(report, writer)?;
    let errors: usize = report.rule_counts.values().map(|count| count.errors).sum();
    let warnings: usize = report
        .rule_counts
        .values()
        .map(|count| count.warnings)
        .sum();
    writeln!(
        writer,
        "{errors} errors, {warnings} warnings, {} suppressed",
        report.suppressed_count
    )?;
    write_failures(report, writer)?;
    write_rule_counts(report, writer)?;
    let ordered = [Severity::Error, Severity::Warning]
        .into_iter()
        .flat_map(|severity| {
            let mut seen = BTreeSet::new();
            let (representatives, remaining): (Vec<_>, Vec<_>) = report
                .diagnostics
                .iter()
                .filter(|item| item.severity == severity)
                .partition(|item| seen.insert(item.rule_id.as_str()));
            representatives.into_iter().chain(remaining)
        });
    for diagnostic in ordered.take(limit) {
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

fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::Full => "full",
        Scope::Selected => "selected",
        Scope::Changed => "changed",
    }
}

fn write_summary(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    let status = match report.status {
        Status::Passed => "PASSED",
        Status::Incomplete => "INCOMPLETE",
        Status::NoChanges => "NO CHANGES",
        Status::Violations => "FAILED",
        Status::ConfigurationError => "CONFIGURATION ERROR",
        Status::EngineError => "ENGINE ERROR",
        Status::PreparationError => "PREPARATION ERROR",
    };
    let scope = scope_label(report.scope);
    write!(
        writer,
        "DornGuard: {status} ({scope}, {} files scanned",
        report.files_scanned
    )?;
    if report.files_requested != report.files_scanned {
        write!(writer, " of {} requested", report.files_requested)?;
    }
    writeln!(writer, ")")?;
    if let (Some(project), Some(organization)) = (report.project_scope, report.organization_scope) {
        writeln!(
            writer,
            "  Project scope: {}. Organization policy scope: {}.",
            scope_label(project),
            scope_label(organization)
        )?;
    }
    if let Some(reason) = &report.fallback_reason {
        writeln!(writer, "Full scan: {reason}")?;
    }
    if let Some(filter) = &report.report_filter {
        writeln!(
            writer,
            "  Report filter omitted {} findings. Counts and exit status cover the full check.",
            filter.omitted
        )?;
    }
    for (label, paths) in [
        (
            "compiled classpath",
            &report.pmd_coverage.compiled_classpath,
        ),
        ("source only", &report.pmd_coverage.source_only),
    ] {
        if let Some(first) = paths.first() {
            writeln!(
                writer,
                "  PMD {label}: {} files (first: {first})",
                paths.len()
            )?;
        }
    }
    for note in &report.analysis_notes {
        writeln!(writer, "  {note}")?;
    }
    Ok(())
}

fn write_rule_counts(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    for (rule, count) in &report.rule_counts {
        writeln!(
            writer,
            "  {rule}: {} errors, {} warnings",
            count.errors, count.warnings
        )?;
    }
    Ok(())
}

fn write_failures(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    for (warning, failure) in report
        .failures
        .iter()
        .map(|failure| (false, failure))
        .chain(
            report
                .analysis_warnings
                .iter()
                .map(|warning| (true, warning)),
        )
    {
        if warning {
            write!(writer, "Warning: ")?;
        }
        match failure.stage {
            Some(FailureStage::Engine) => {
                if let Some(engine) = failure.engine {
                    write!(writer, "  {} analysis: ", engine.executable())?;
                }
            }
            Some(FailureStage::ProjectModel) => {
                write!(
                    writer,
                    "  {} project model: ",
                    failure.build_tool.unwrap_or("Build")
                )?;
            }
            Some(FailureStage::ProjectCompile) => {
                write!(
                    writer,
                    "  {} compile and model export: ",
                    failure.build_tool.unwrap_or("Build")
                )?;
            }
            None => write!(writer, "  ")?,
        }
        if let Some(path) = &failure.path {
            write!(writer, "{path}: ")?;
        }
        if let Some(kind) = &failure.error_type {
            write!(writer, "{kind}: ")?;
        }
        writeln!(writer, "{}", failure.message)?;
        if !failure.modules.is_empty() {
            writeln!(writer, "    Modules: {}", failure.modules.join(", "))?;
        }
        if let Some(count) = failure.file_count {
            writeln!(writer, "    Files in this analysis: {count}")?;
        }
        if !failure.rule_ids.is_empty() {
            writeln!(
                writer,
                "    Rules with incomplete results: {}",
                failure.rule_ids.join(", ")
            )?;
        }
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
