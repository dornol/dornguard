# Profiles and rulesets

Run `dornguard profiles list` to see available profiles and
`dornguard profiles show NAME` for their direct rules and parent profiles.
Use `dornguard rules explain ID` for the reason and guidance for a rule.

| Profile | Checks |
| --- | --- |
| `java-source` | Semgrep Java source rules |
| `java` | Semgrep and PMD Java source rules |
| `kotlin` | Semgrep rules for Gradle Kotlin/JVM production sources |
| `spring-boot` | Java rules and Spring injection checks |
| `thymeleaf` | Spring bean references in restricted template attributes |
| `security` | Java CORS patterns |
| `hexagonal` | Domain, application, and adapter dependencies |
| `layered` | Controller, service, and repository dependencies |
| `rest-api` | Spring and security rules |
| `scheduler` | Spring rules for scheduler modules |
| `java-library` | Java and framework independence rules |

## Thymeleaf compatibility

Add `thymeleaf` to `profiles` to scan `.html` and `.htm` files under
`src/main/resources/templates` in the project or its build modules.
`thymeleaf.restricted-bean-access` flags Spring bean references in direct
expressions in `th:href`, `th:src`, `th:utext`, `th:on*`, `th:insert`, and
`th:replace`, including their `data-th-*` forms. It also checks simple
dynamic URL bases and fragment selectors. These restricted expressions
can fail at render time on Thymeleaf 3.1.4 and later. Compute the value in
the controller and expose it as a model attribute.

## Reuse a policy bundle

A policy bundle has a `ruleset.yml` manifest. The manifest names files
relative to the bundle directory.

```yaml
version: 1
profiles: [profiles/team-java.yml]
rules: [rules/team.log-stderr.yml]
```

Profiles declare `name`, `description`, `extends`, `rules`, and optional
`rule_defaults` using the fields in the configuration schema. Custom rule
metadata can bind one Semgrep pattern rule to its engine file or provide
package selectors for ArchUnit. Rule IDs, engine rule IDs and profile names
must be unique.
Declare the rule's source languages in its Semgrep YAML. A rule targeting
Kotlin activates Gradle Kotlin/JVM source discovery for its policy scope.

Set `ruleset: { source: local, path: .dornguard-policy }` to load a bundle
inside the project, or use a versioned Git repository.

```yaml
ruleset:
  source: git
  repository: https://github.com/example/team-policy
  ref: v1.2.0
```

Review and commit `.dornguard-ruleset.lock.json` after resolving a tag.
Subsequent checks use the recorded commit and reuse the local Git object
cache. A full commit SHA can also be configured directly. Use
`DORNGUARD_CACHE_DIR` to move the cache.
