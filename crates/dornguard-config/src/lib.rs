use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use dornguard_core::{EngineName, Severity, SourceLanguage};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::Deserialize;

mod ruleset;
use ruleset::{PolicySource, Ruleset, Source};

pub const CONFIG_FILE: &str = ".dornguard.yml";
pub const DEFAULT_CONFIG: &str = "version: 1\nprofiles: [java]\n";
pub const BUILTIN_VERSION: &str = "1.0.0";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    version: u32,
    #[serde(default)]
    profiles: Vec<String>,
    #[serde(default)]
    ruleset: Ruleset,
    #[serde(default)]
    pub modules: BTreeMap<String, ModuleConfig>,
    #[serde(default)]
    pub module_dependencies: ModuleDependencies,
    #[serde(default)]
    rules: BTreeMap<String, RuleOverride>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleConfig {
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    rules: BTreeMap<String, RuleOverride>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleDependencies {
    #[serde(default)]
    pub forbid: Vec<Dependency>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub extends: Vec<String>,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub rule_defaults: BTreeMap<String, RuleOverride>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleOverride {
    enabled: Option<bool>,
    severity: Option<Severity>,
    options: Option<ArchitectureOptions>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub engine: EngineName,
    pub severity: Severity,
    pub description: String,
    pub fix: String,
    implementation: Option<Implementation>,
    pub options: Option<ArchitectureOptions>,
    #[serde(skip)]
    semgrep_languages: Vec<String>,
}

impl Rule {
    pub fn applies_to(&self, language: SourceLanguage) -> bool {
        match self.engine {
            EngineName::Pmd => language == SourceLanguage::Java,
            EngineName::Semgrep => self.semgrep_languages.iter().any(|name| {
                name == language.semgrep_name() || matches!(name.as_str(), "generic" | "regex")
            }),
            _ => false,
        }
    }

    pub fn enables_kotlin(&self) -> bool {
        self.semgrep_languages.iter().any(|name| name == "kotlin")
    }

    pub fn implementation(&self) -> Result<&Implementation> {
        self.implementation
            .as_ref()
            .ok_or_else(|| miette!("Rule {} is missing its engine implementation", self.id))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureOptions {
    #[serde(default)]
    pub from: Vec<String>,
    pub forbidden: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Implementation {
    pub config: String,
    pub rule_id: String,
}

pub struct Registry {
    profiles: BTreeMap<String, Profile>,
    rules: BTreeMap<String, Rule>,
    engine_configs: BTreeMap<String, String>,
}

impl Registry {
    pub fn builtin() -> Result<Self> {
        let profile_sources = [
            include_str!("../assets/profiles/java-source.yml"),
            include_str!("../assets/profiles/java.yml"),
            include_str!("../assets/profiles/kotlin.yml"),
            include_str!("../assets/profiles/security.yml"),
            include_str!("../assets/profiles/spring-boot.yml"),
            include_str!("../assets/profiles/hexagonal.yml"),
            include_str!("../assets/profiles/layered.yml"),
            include_str!("../assets/profiles/java-library.yml"),
            include_str!("../assets/profiles/rest-api.yml"),
            include_str!("../assets/profiles/scheduler.yml"),
        ];
        let rule_sources = [
            include_str!("../assets/rules/java.stdout-usage.yml"),
            include_str!("../assets/rules/java.empty-catch.yml"),
            include_str!("../assets/rules/kotlin.stdout-usage.yml"),
            include_str!("../assets/rules/security.cors-wildcard.yml"),
            include_str!("../assets/rules/spring.field-injection.yml"),
            include_str!("../assets/rules/architecture.domain-independence.yml"),
            include_str!("../assets/rules/architecture.application-independence.yml"),
            include_str!("../assets/rules/architecture.adapter-isolation.yml"),
            include_str!("../assets/rules/architecture.framework-independence.yml"),
            include_str!("../assets/rules/architecture.controller-repository.yml"),
            include_str!("../assets/rules/architecture.repository-independence.yml"),
            include_str!("../assets/rules/architecture.module-dependency.yml"),
            include_str!("../assets/rules/architecture.library-framework-independence.yml"),
        ];
        let engine_configs = BTreeMap::from([
            (
                "rules/semgrep/java/stdout-usage.yml",
                include_str!("../assets/rules/semgrep/java/stdout-usage.yml"),
            ),
            (
                "rules/pmd/java/empty-catch.xml",
                include_str!("../assets/rules/pmd/java/empty-catch.xml"),
            ),
            (
                "rules/semgrep/kotlin/stdout-usage.yml",
                include_str!("../assets/rules/semgrep/kotlin/stdout-usage.yml"),
            ),
            (
                "rules/semgrep/security/cors-wildcard.yml",
                include_str!("../assets/rules/semgrep/security/cors-wildcard.yml"),
            ),
            (
                "rules/semgrep/spring/field-injection.yml",
                include_str!("../assets/rules/semgrep/spring/field-injection.yml"),
            ),
        ]);
        let mut registry = Self {
            profiles: BTreeMap::new(),
            rules: BTreeMap::new(),
            engine_configs: engine_configs
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        };
        for source in profile_sources {
            let profile: Profile = serde_saphyr::from_str(source)
                .into_diagnostic()
                .wrap_err("Invalid built-in profile")?;
            registry.insert_profile(profile)?;
        }

        for source in rule_sources {
            let rule: Rule = serde_saphyr::from_str(source)
                .into_diagnostic()
                .wrap_err("Invalid built-in rule")?;
            registry.insert_rule(rule)?;
        }
        Ok(registry)
    }

    pub fn for_config(path: &Path, config: &ProjectConfig) -> Result<Self> {
        let mut registry = Self::builtin()?;
        if let Some(source) = Source::open(path, &config.ruleset)? {
            registry.extend(source)?;
        }
        Ok(registry)
    }

    fn extend(&mut self, source: Source) -> Result<()> {
        let manifest = source.manifest()?;
        for path in manifest.rules {
            let rule: Rule = source.parse(&path)?;
            match rule.engine {
                EngineName::Semgrep => {
                    let implementation = rule.implementation()?;
                    let path = &implementation.config;
                    if self.engine_configs.contains_key(path) {
                        bail!("Duplicate engine configuration path: {path}");
                    }
                    let contents = source.read_file(path)?;
                    self.engine_configs.insert(path.clone(), contents);
                }
                EngineName::Archunit => {}
                EngineName::Pmd | EngineName::Gradle => {
                    bail!("Custom rules support Semgrep patterns and ArchUnit package checks");
                }
            }
            self.insert_rule(rule)?;
        }
        for path in manifest.profiles {
            self.insert_profile(source.parse(&path)?)?;
        }
        for profile in self.profiles.values() {
            self.selected_profiles([&profile.name])?;
            for id in &profile.rules {
                self.rule(id)?;
            }
            self.validate_overrides(&profile.rule_defaults)?;
        }
        Ok(())
    }

    fn insert_profile(&mut self, profile: Profile) -> Result<()> {
        let name = profile.name.clone();
        if name.is_empty() || self.profiles.insert(name.clone(), profile).is_some() {
            bail!("Empty or duplicate profile name: {name}");
        }
        Ok(())
    }

    fn insert_rule(&mut self, mut rule: Rule) -> Result<()> {
        let id = rule.id.clone();
        match rule.engine {
            EngineName::Semgrep | EngineName::Pmd => {
                let implementation = rule.implementation()?;
                let Some(contents) = self.engine_configs.get(&implementation.config) else {
                    bail!("Rule {id} is missing its engine configuration");
                };
                if self.rules.values().any(|other| {
                    other.engine == rule.engine
                        && other
                            .implementation
                            .as_ref()
                            .is_some_and(|other| other.rule_id == implementation.rule_id)
                }) {
                    bail!("Duplicate engine rule ID: {}", implementation.rule_id);
                }
                if rule.engine == EngineName::Semgrep {
                    let path = &implementation.config;
                    let semgrep: SemgrepRules = serde_saphyr::from_str(contents)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("Invalid Semgrep configuration {path}"))?;
                    let [configured] = semgrep.rules.as_slice() else {
                        bail!(
                            "Semgrep configuration {path} must contain exactly the declared engine rule"
                        );
                    };
                    if configured.id != implementation.rule_id || configured.languages.is_empty() {
                        bail!(
                            "Semgrep configuration {path} must declare the engine rule ID and its languages"
                        );
                    }
                    rule.semgrep_languages = configured.languages.clone();
                }
            }
            EngineName::Archunit
                if rule
                    .options
                    .as_ref()
                    .is_none_or(|options| options.forbidden.is_empty()) =>
            {
                bail!("Rule {id} requires architecture options with forbidden packages");
            }
            EngineName::Archunit | EngineName::Gradle => {}
        }
        if id.is_empty() || self.rules.insert(id.clone(), rule).is_some() {
            bail!("Empty or duplicate rule ID: {id}");
        }
        Ok(())
    }

    pub fn profiles(&self) -> impl Iterator<Item = &Profile> {
        self.profiles.values()
    }

    pub fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.rules.values()
    }

    pub fn profile(&self, name: &str) -> Result<&Profile> {
        self.profiles
            .get(name)
            .ok_or_else(|| miette!("Unknown profile: {name}"))
    }

    pub fn rule(&self, id: &str) -> Result<&Rule> {
        self.rules
            .get(id)
            .ok_or_else(|| miette!("Unknown rule: {id}"))
    }

    pub fn engine_config(&self, rule: &Rule) -> Result<&str> {
        self.engine_configs
            .get(rule.implementation()?.config.as_str())
            .map(String::as_str)
            .ok_or_else(|| miette!("Missing engine configuration for {}", rule.id))
    }

    pub fn load(path: &Path) -> Result<ProjectConfig> {
        let source = fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Could not read {}", path.display()))?;
        let config: ProjectConfig = serde_saphyr::from_str(&source)
            .into_diagnostic()
            .wrap_err_with(|| format!("Invalid configuration in {}", path.display()))?;
        if config.version != 1 {
            bail!(
                "Unsupported configuration version {}; expected 1",
                config.version
            );
        }
        config.ruleset.validate()?;
        Ok(config)
    }

    pub fn resolve(
        &self,
        config: &ProjectConfig,
        module: Option<&str>,
        extra_profiles: &[String],
    ) -> Result<Vec<Rule>> {
        let module = module.and_then(|module| config.modules.get(module));
        let profiles = self.selected_profiles(
            config
                .profiles
                .iter()
                .chain(module.into_iter().flat_map(|module| &module.profiles))
                .chain(extra_profiles),
        )?;
        let explicit = module
            .into_iter()
            .map(|module| &module.rules)
            .chain([&config.rules])
            .collect::<Vec<_>>();
        for overrides in &explicit {
            self.validate_overrides(overrides)?;
        }
        let ids = profiles
            .iter()
            .flat_map(|profile| &profile.rules)
            .collect::<BTreeSet<_>>();
        let mut active = Vec::new();
        for id in ids {
            let mut rule = self.rule(id)?.clone();
            let explicit = explicit
                .iter()
                .filter_map(|overrides| overrides.get(id))
                .collect::<Vec<_>>();
            let defaults = profiles
                .iter()
                .filter_map(|profile| {
                    profile
                        .rule_defaults
                        .get(id)
                        .map(|value| (profile.name.as_str(), value))
                })
                .collect::<Vec<_>>();
            let enabled = effective(id, "enabled", &explicit, &defaults, |item| {
                item.enabled.as_ref()
            })?
            .unwrap_or(&true);
            let severity = effective(id, "severity", &explicit, &defaults, |item| {
                item.severity.as_ref()
            })?;
            let options = effective(id, "options", &explicit, &defaults, |item| {
                item.options.as_ref()
            })?;
            if *enabled {
                if let Some(severity) = severity {
                    rule.severity = *severity;
                }
                if let Some(options) = options {
                    rule.options = Some(options.clone());
                }
                active.push(rule);
            }
        }
        Ok(active)
    }

    pub fn kotlin_profile_enabled(
        &self,
        config: &ProjectConfig,
        module: Option<&str>,
        extra_profiles: &[String],
    ) -> Result<bool> {
        let module = module.and_then(|name| config.modules.get(name));
        let profiles = config
            .profiles
            .iter()
            .chain(module.into_iter().flat_map(|module| &module.profiles))
            .chain(extra_profiles);
        Ok(self
            .selected_profiles(profiles)?
            .iter()
            .any(|profile| profile.name == "kotlin"))
    }

    fn validate_overrides(&self, overrides: &BTreeMap<String, RuleOverride>) -> Result<()> {
        for (id, value) in overrides {
            let rule = self.rule(id)?;
            if let Some(options) = &value.options
                && (rule.engine != EngineName::Archunit || options.forbidden.is_empty())
            {
                bail!("Rule {id} does not accept these options");
            }
            if rule.engine == EngineName::Gradle {
                bail!("Configure Gradle dependency rules through module_dependencies");
            }
        }
        Ok(())
    }

    fn selected_profiles<'a>(
        &self,
        profiles: impl IntoIterator<Item = &'a String>,
    ) -> Result<Vec<&Profile>> {
        let mut done = BTreeSet::new();
        let mut stack = Vec::new();
        let mut selected = Vec::new();
        for name in profiles {
            self.visit(name, &mut stack, &mut done, &mut selected)?;
        }
        Ok(selected)
    }

    fn visit<'a>(
        &'a self,
        name: &str,
        stack: &mut Vec<&'a str>,
        done: &mut BTreeSet<&'a str>,
        selected: &mut Vec<&'a Profile>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        let profile = self.profile(name)?;
        if stack.contains(&profile.name.as_str()) {
            stack.push(&profile.name);
            bail!("Profile inheritance cycle: {}", stack.join(" -> "));
        }
        stack.push(&profile.name);
        for parent in &profile.extends {
            self.visit(parent, stack, done, selected)?;
        }
        selected.push(profile);
        stack.pop();
        done.insert(&profile.name);
        Ok(())
    }
}

fn effective<'a, T: PartialEq + 'a>(
    id: &str,
    option: &str,
    explicit: &[&'a RuleOverride],
    defaults: &[(&str, &'a RuleOverride)],
    field: impl Fn(&'a RuleOverride) -> Option<&'a T>,
) -> Result<Option<&'a T>> {
    if let Some(value) = explicit.iter().find_map(|value| field(value)) {
        return Ok(Some(value));
    }
    let mut values = defaults
        .iter()
        .filter_map(|(name, value)| field(value).map(|value| (*name, value)));
    let first = values.next();
    if let Some((name, value)) = first
        && let Some((other, _)) = values.find(|(_, other)| other != &value)
    {
        bail!("Rule {id} has conflicting {option} defaults in profiles {name} and {other}");
    }
    Ok(first.map(|(_, value)| value))
}

#[derive(Deserialize)]
struct SemgrepRules {
    rules: Vec<SemgrepRuleId>,
}

#[derive(Deserialize)]
struct SemgrepRuleId {
    id: String,
    languages: Vec<String>,
}

impl ProjectConfig {
    pub fn validate_trusted_ruleset(&self, policy: &Path, worktree: &Path) -> Result<()> {
        self.ruleset.validate_trust(policy, worktree)
    }

    pub fn local_ruleset_path(&self) -> Option<&Path> {
        if self.ruleset.source == PolicySource::Local {
            self.ruleset.path.as_deref()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_inheritance_deduplicates_rules_and_reports_cycles() {
        let mut registry = Registry::builtin().unwrap();
        registry.profiles.insert(
            "extra".into(),
            Profile {
                name: "extra".into(),
                description: String::new(),
                extends: vec!["java".into()],
                rules: vec!["java.stdout-usage".into()],
                rule_defaults: BTreeMap::new(),
            },
        );
        let selected = ["extra".into(), "java".into()];
        let config: ProjectConfig =
            serde_saphyr::from_str("version: 1\nprofiles: [extra, java]\n").unwrap();
        assert_eq!(registry.resolve(&config, None, &[]).unwrap().len(), 2);
        registry
            .profiles
            .get_mut("java")
            .unwrap()
            .extends
            .push("extra".into());
        assert!(
            registry
                .selected_profiles(&selected)
                .unwrap_err()
                .to_string()
                .contains("extra -> java -> extra")
        );
    }

    #[test]
    fn explicit_project_and_module_fields_resolve_profile_conflicts() {
        let mut registry = Registry::builtin().unwrap();
        for (name, severity) in [("first", "warning"), ("second", "error")] {
            let profile = serde_saphyr::from_str(&format!(
                "name: {name}\ndescription: test\nrules: [java.stdout-usage]\nrule_defaults:\n  java.stdout-usage: {{severity: {severity}}}\n"
            )).unwrap();
            registry.insert_profile(profile).unwrap();
        }
        let conflicting =
            serde_saphyr::from_str("version: 1\nprofiles: [first, second]\n").unwrap();
        let error = registry.resolve(&conflicting, None, &[]).unwrap_err();
        assert!(error.to_string().contains("profiles first and second"));

        let project = serde_saphyr::from_str(
            r#"
version: 1
profiles: [first, second]
rules:
  java.stdout-usage: { severity: error }
modules:
  ":api":
    rules:
      java.stdout-usage: { severity: warning }
  ":admin":
    rules:
      java.stdout-usage: { enabled: false }
"#,
        )
        .unwrap();
        assert_eq!(
            registry.resolve(&project, None, &[]).unwrap()[0].severity,
            Severity::Error
        );
        assert_eq!(
            registry.resolve(&project, Some(":api"), &[]).unwrap()[0].severity,
            Severity::Warning
        );
        assert!(
            registry
                .resolve(&project, Some(":admin"), &[])
                .unwrap()
                .is_empty()
        );
    }
}
