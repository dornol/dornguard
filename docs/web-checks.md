# HTML, JavaScript, and CSP checks

Enable the web profiles alongside the existing project profiles.

```yaml
version: 1
profiles: [spring-boot, thymeleaf, html, javascript]
```

The `html` profile warns about executable inline script blocks and common
inline event attributes such as `onclick`, `onerror`, and the Thymeleaf
`th:on*` and `data-th-on*` forms. Separate scripts loaded through `src` or
`th:src` and data blocks such as `application/ld+json` are skipped.

The `javascript` profile requires ESLint 9 or 10. DornGuard prefers
`node_modules/.bin/eslint` in the project, then PATH, then the copy that
`dornguard engines install eslint` installs. It
checks `.js`, `.mjs`, and `.cjs` with `no-eval`, `no-implied-eval`,
`no-new-func`, and `no-async-promise-executor`. It uses its own ESLint config
and reports those findings as warnings unless the project overrides their severity. Files under
`node_modules` are excluded. [ESLint setup](https://eslint.org/docs/latest/use/getting-started)

When a Maven or Gradle build is active, HTML and JavaScript are discovered
under each selected module’s `src/main/webapp`, `src/main/resources/static`,
`src/main/resources/public`, and template directory. JavaScript is also
discovered under `src/main/js` and `src/main/frontend`.

## JSP

Add `jsp` to the project's profiles to warn about JSP scripting elements
such as `<% ... %>` and `<jsp:expression>`, and `<c:out>` tags with a
literal `escapeXml="false"`. It scans `.jsp`, `.jspx`, `.jspf`, `.tag`, and
`.tagx` files. In Maven and Gradle build checks, JSP sources are discovered
under each selected module's `src/main/webapp`.

## Inspect a deployed response

```sh
dornguard csp https://staging.example.com/books --format json
```

This command uses curl to fetch one successful UTF-8 HTML response.
It distinguishes enforced CSP headers from Report-Only headers and
checks for missing script restrictions and effective `unsafe-inline`
and `unsafe-eval` permissions. It compares each executable inline
script’s nonce or content hash with every enforcing policy. The report
identifies blocked inline scripts by their order in the response.
The runtime scope is the response headers and the authorization of its
inline script blocks.

Exit code `0` means the checks passed, `1` means findings were reported,
and `2` means the command could not complete. Use the final page URL
when the server redirects. [CSP Level 3](https://www.w3.org/TR/CSP3/)

## Partial Thymeleaf parsing

DornGuard reports Semgrep partial parsing as incomplete analysis. To let a
project check exit successfully when the only analysis problems are partial
parses for Thymeleaf rules, set the following option.

```yaml
thymeleaf_partial_parsing: warning
```

The report still has `complete: false` and lists the affected files and
rules in `analysis_warnings`. The status is `incomplete` with exit code 0
when there are no blocking findings or other failures. Confirmed violations
and incomplete results from HTML or organization rules still block the check.
Baseline creation requires complete analysis. See
[Configuration](configuration.md#incomplete-analysis-of-valid-code) for
`partial_parsing` and `partial_parsing_files`, which cover all languages.
