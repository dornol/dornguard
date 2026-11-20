use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::process::Command;

use crate::{TextOrJson, maven};

const DEPENDENCY_TREE: &str = "org.apache.maven.plugins:maven-dependency-plugin:3.11.0:tree";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MavenNode {
    group_id: String,
    artifact_id: String,
    version: String,
    #[serde(rename = "type")]
    kind: String,
    scope: String,
    #[serde(default)]
    children: Vec<Self>,
}

impl MavenNode {
    fn coordinate(&self) -> Result<(String, String)> {
        let valid_id = |part: &str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        };
        if !valid_id(&self.group_id)
            || !valid_id(&self.artifact_id)
            || self.version.is_empty()
            || self.version.chars().any(char::is_control)
        {
            bail!("Maven dependency tree contains an invalid coordinate");
        }
        Ok((
            format!("{}:{}", self.group_id, self.artifact_id),
            self.version.clone(),
        ))
    }
}

type Inventory = BTreeMap<(String, String), BTreeSet<String>>;

struct Reactor {
    modules: BTreeSet<String>,
    packages: Inventory,
}

impl Reactor {
    fn parse(data: &[u8]) -> Result<Self> {
        let mut projects = serde_json::Deserializer::from_slice(data).into_iter::<MavenNode>();
        let roots = projects
            .by_ref()
            .collect::<serde_json::Result<Vec<_>>>()
            .into_diagnostic()
            .wrap_err("Invalid Maven dependency tree")?;
        if roots.is_empty() {
            bail!("Maven returned an empty dependency tree");
        }
        let internal = roots
            .iter()
            .map(MavenNode::coordinate)
            .collect::<Result<BTreeSet<_>>>()?;
        let mut modules = BTreeSet::new();
        let mut packages = Inventory::new();
        for root in roots {
            let (name, _) = root.coordinate()?;
            if !modules.insert(name.clone()) {
                bail!("Multiple Maven reactor modules use the same coordinate: {name}");
            }
            let mut pending = root.children;
            while let Some(node) = pending.pop() {
                if !matches!(node.scope.as_str(), "compile" | "runtime") {
                    continue;
                }
                let coordinate = node.coordinate()?;
                if node.kind != "pom" && !internal.contains(&coordinate) {
                    packages.entry(coordinate).or_default().insert(name.clone());
                }
                pending.extend(node.children);
            }
        }
        Ok(Self { modules, packages })
    }

    fn sbom(&self) -> serde_json::Value {
        let components: Vec<_> = self
            .packages
            .keys()
            .filter_map(|(name, version)| {
                // `MavenNode::coordinate` only admits IDs without ':'.
                let (group, artifact) = name.split_once(':')?;
                let purl = format!("pkg:maven/{group}/{artifact}@{}", purl_version(version));
                Some(json!({
                    "type": "library",
                    "group": group,
                    "name": artifact,
                    "version": version,
                    "purl": purl,
                }))
            })
            .collect();
        json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.6",
            "version": 1,
            "components": components,
        })
    }

    fn findings(&self, data: &[u8]) -> Result<Vec<Finding>> {
        let scanned: ScannerResult = serde_json::from_slice(data)
            .into_diagnostic()
            .wrap_err("Invalid OSV-Scanner JSON report")?;
        let mut seen = BTreeSet::new();
        let mut findings = BTreeSet::new();
        for result in scanned.results {
            for item in result.packages {
                let package = item.package;
                let coordinate = (package.name, package.version);
                let Some(modules) = self.packages.get(&coordinate) else {
                    bail!("OSV-Scanner returned a package outside the Maven inventory");
                };
                if package.ecosystem != "Maven" {
                    bail!("OSV-Scanner returned an unexpected ecosystem");
                }
                for advisory in item.vulnerabilities {
                    if advisory.id.is_empty() {
                        bail!("OSV-Scanner returned an advisory without an ID");
                    }
                    findings.insert(Finding {
                        package: coordinate.0.clone(),
                        version: coordinate.1.clone(),
                        modules: modules.clone(),
                        advisory,
                    });
                }
                seen.insert(coordinate);
            }
        }
        if seen.len() != self.packages.len() {
            bail!(
                "OSV-Scanner reported coverage for {} of {} resolved Maven packages",
                seen.len(),
                self.packages.len()
            );
        }
        Ok(findings.into_iter().collect())
    }
}

fn purl_version(version: &str) -> String {
    let mut encoded = String::new();
    for byte in version.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    encoded
}

#[derive(Deserialize)]
struct ScannerResult {
    results: Vec<ScannerSource>,
}

#[derive(Deserialize)]
struct ScannerSource {
    packages: Vec<ScannerPackage>,
}

#[derive(Deserialize)]
struct ScannerPackage {
    package: ScannedCoordinate,
    #[serde(default)]
    vulnerabilities: Vec<Advisory>,
}

#[derive(Deserialize)]
struct ScannedCoordinate {
    name: String,
    version: String,
    ecosystem: String,
}

#[derive(Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
struct Advisory {
    id: String,
    #[serde(default)]
    aliases: BTreeSet<String>,
    #[serde(default)]
    summary: String,
}

#[derive(Serialize, Eq, Ord, PartialEq, PartialOrd)]
struct Finding {
    package: String,
    version: String,
    modules: BTreeSet<String>,
    #[serde(flatten)]
    advisory: Advisory,
}

#[derive(Serialize)]
struct Report {
    version: u8,
    status: &'static str,
    modules: BTreeSet<String>,
    packages_scanned: usize,
    findings: Vec<Finding>,
}

pub async fn scan(project: &Path, profiles: &[String], format: TextOrJson) -> Result<u8> {
    let root = dunce::canonicalize(project)
        .into_diagnostic()
        .wrap_err("Could not open the Maven project")?;
    if !root.join("pom.xml").is_file() {
        bail!("Run the dependency audit in a Maven project directory containing pom.xml");
    }
    let working = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Could not create the dependency audit workspace")?;
    let trees = working.path().join("dependencies.json");
    let mut output_option = OsString::from("-DoutputFile=");
    output_option.push(&trees);
    let mut command = maven::command(&root);
    command.args([
        "--threads",
        "1",
        "--fail-fast",
        DEPENDENCY_TREE,
        "-DoutputType=json",
        "-Dscope=runtime",
        "-DappendOutput=true",
        "-Dverbose=false",
    ]);
    command.arg(output_option);
    if !profiles.is_empty() {
        command.arg(format!("-P{}", profiles.join(",")));
    }
    maven::run(&mut command).await?;
    let data = std::fs::read(trees)
        .into_diagnostic()
        .wrap_err("Maven did not write the resolved dependency tree")?;
    let reactor = Reactor::parse(&data)?;
    let findings = if reactor.packages.is_empty() {
        Vec::new()
    } else {
        audit_inventory(&reactor, working.path()).await?
    };

    let exit_code = u8::from(!findings.is_empty());
    let report = Report {
        version: 1,
        status: if exit_code == 0 {
            "passed"
        } else {
            "vulnerabilities"
        },
        modules: reactor.modules,
        packages_scanned: reactor.packages.len(),
        findings,
    };
    write_report(&report, format, &mut io::stdout().lock())?;
    Ok(exit_code)
}

async fn audit_inventory(reactor: &Reactor, working: &Path) -> Result<Vec<Finding>> {
    let scan_dir = working.join("sbom");
    std::fs::create_dir(&scan_dir).into_diagnostic()?;
    std::fs::write(
        scan_dir.join("dependencies.cdx.json"),
        serde_json::to_vec(&reactor.sbom()).into_diagnostic()?,
    )
    .into_diagnostic()
    .wrap_err("Could not write the CycloneDX inventory")?;

    let output = tokio::time::timeout(
        Duration::from_secs(120),
        Command::new("osv-scanner")
            .current_dir(scan_dir)
            .kill_on_drop(true)
            .args([
                "scan",
                "source",
                "--all-packages",
                "--format=json",
                "--no-resolve",
                "--verbosity=error",
                ".",
            ])
            .output(),
    )
    .await
    .into_diagnostic()
    .wrap_err("OSV-Scanner timed out")?
    .into_diagnostic()
    .wrap_err("Could not start OSV-Scanner. Install OSV-Scanner v2 on PATH")?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("OSV-Scanner failed ({})\n{}", output.status, stderr.trim());
    }
    let findings = reactor.findings(&output.stdout)?;
    if output.status.success() != findings.is_empty() {
        bail!("OSV-Scanner exit status disagrees with its vulnerability report");
    }
    Ok(findings)
}

fn write_report(report: &Report, format: TextOrJson, output: &mut impl Write) -> Result<()> {
    match format {
        TextOrJson::Json => {
            serde_json::to_writer_pretty(&mut *output, report).into_diagnostic()?;
            writeln!(output).into_diagnostic()?;
        }
        TextOrJson::Text => {
            writeln!(
                output,
                "DornGuard audit: {} ({} modules, {} packages checked, {} findings)",
                report.status,
                report.modules.len(),
                report.packages_scanned,
                report.findings.len()
            )
            .into_diagnostic()?;
            for item in &report.findings {
                let modules = item.modules.iter().map(String::as_str).collect::<Vec<_>>();
                writeln!(
                    output,
                    "{}:{} {} [{}]",
                    item.package,
                    item.version,
                    item.advisory.id,
                    modules.join(", ")
                )
                .into_diagnostic()?;
                if !item.advisory.summary.is_empty() {
                    writeln!(output, "  {}", item.advisory.summary).into_diagnostic()?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory() -> Reactor {
        Reactor::parse(
            br#"{"groupId":"acme","artifactId":"root","version":"1","type":"pom","scope":""}
{"groupId":"acme","artifactId":"app","version":"1","type":"jar","scope":"","children":[
  {"groupId":"acme","artifactId":"lib","version":"1","type":"jar","scope":"compile","children":[
    {"groupId":"external","artifactId":"dep","version":"1.0+runtime","type":"jar","scope":"runtime"}
  ]},
  {"groupId":"external","artifactId":"test-kit","version":"1","type":"jar","scope":"test"}
]}
{"groupId":"acme","artifactId":"lib","version":"1","type":"jar","scope":"","children":[
  {"groupId":"external","artifactId":"dep","version":"1.0+runtime","type":"jar","scope":"runtime"}
]}"#,
        )
        .unwrap()
    }

    #[test]
    fn reactor_inventory_associates_exact_runtime_versions_with_modules() {
        let reactor = inventory();
        assert_eq!(reactor.modules.len(), 3);
        assert_eq!(reactor.packages.len(), 1);
        assert_eq!(
            reactor.packages[&("external:dep".to_owned(), "1.0+runtime".to_owned())],
            BTreeSet::from(["acme:app".to_owned(), "acme:lib".to_owned()]),
        );
        assert_eq!(
            reactor.sbom()["components"][0]["purl"],
            "pkg:maven/external/dep@1.0%2Bruntime"
        );
    }

    #[test]
    fn scanner_advisories_are_mapped_and_partial_coverage_fails() {
        let reactor = inventory();
        let data = br#"{"results":[{"packages":[{"package":{
          "name":"external:dep","version":"1.0+runtime","ecosystem":"Maven"
        },"vulnerabilities":[{"id":"DEMO-ADVISORY","aliases":["DEMO-ALIAS"]}]}]}]}"#;
        let findings = reactor.findings(data).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].advisory.id, "DEMO-ADVISORY");
        assert_eq!(findings[0].modules.len(), 2);
        assert!(reactor.findings(br#"{"results":[]}"#).is_err());
        assert!(Reactor::parse(b"").is_err());
        assert!(Reactor::parse(b"{}").is_err());
    }
}
