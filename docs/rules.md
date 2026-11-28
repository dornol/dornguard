# Rule catalog

<!-- Generated from the built-in rules. Regenerate with
DORNGUARD_BLESS=1 cargo test -p dornguard-config rule_catalog -->

Every built-in rule with what it reports, an example, the fix and when
suppression is allowed. `dornguard rules explain RULE...` prints the same
guidance, and `--format json` returns it as data. Severity is the default;
`medium` confidence means the rule infers intent from code shape, so
confirm a finding before fixing it.

| Rule | Engine | Severity | Confidence | Profiles |
| --- | --- | --- | --- | --- |
| [`architecture.adapter-isolation`](#architectureadapter-isolation) | archunit | error | high | `hexagonal` |
| [`architecture.application-independence`](#architectureapplication-independence) | archunit | error | high | `hexagonal` |
| [`architecture.controller-repository`](#architecturecontroller-repository) | archunit | error | high | `layered` |
| [`architecture.domain-independence`](#architecturedomain-independence) | archunit | error | high | `hexagonal` |
| [`architecture.framework-independence`](#architectureframework-independence) | archunit | error | high | `hexagonal` |
| [`architecture.library-framework-independence`](#architecturelibrary-framework-independence) | archunit | error | high | `java-library` |
| [`architecture.module-dependency`](#architecturemodule-dependency) | gradle | error | high | `module_dependencies` |
| [`architecture.package-cycle`](#architecturepackage-cycle) | archunit | error | high | `package-cycles` |
| [`architecture.repository-independence`](#architecturerepository-independence) | archunit | error | high | `layered` |
| [`architecture.service-independence`](#architectureservice-independence) | archunit | error | high | `layered` |
| [`flyway.changed-versioned-migration`](#flywaychanged-versioned-migration) | builtin | warning | high | `flyway` |
| [`format.blank-lines`](#formatblank-lines) | builtin | error | high | `format` |
| [`format.control-spacing`](#formatcontrol-spacing) | builtin | error | high | `format` |
| [`format.final-newline`](#formatfinal-newline) | builtin | error | high | `format` |
| [`format.indentation`](#formatindentation) | builtin | error | high | `format` |
| [`format.line-ending`](#formatline-ending) | builtin | error | high | `format` |
| [`format.trailing-whitespace`](#formattrailing-whitespace) | builtin | error | high | `format` |
| [`go.vet`](#govet) | go_vet | error | high | `go` |
| [`html.inline-event-handler`](#htmlinline-event-handler) | semgrep | warning | high | `html` |
| [`html.inline-script`](#htmlinline-script) | semgrep | warning | high | `html` |
| [`java.assert-throws-nested-call`](#javaassert-throws-nested-call) | semgrep | warning | medium | `java-review` |
| [`java.bigdecimal-from-floating-point-literal`](#javabigdecimal-from-floating-point-literal) | pmd | warning | high | `java` |
| [`java.broken-null-check`](#javabroken-null-check) | pmd | warning | medium | `java` |
| [`java.cognitive-complexity`](#javacognitive-complexity) | pmd | warning | medium | `java-design` |
| [`java.date-pattern-typo`](#javadate-pattern-typo) | semgrep | error | high | `java-source` |
| [`java.double-checked-locking`](#javadouble-checked-locking) | pmd | warning | high | `java` |
| [`java.empty-catch`](#javaempty-catch) | pmd | error | high | `java` |
| [`java.equals-hashcode-contract`](#javaequals-hashcode-contract) | pmd | warning | high | `java` |
| [`java.excessive-parameters`](#javaexcessive-parameters) | pmd | warning | medium | `java-design` |
| [`java.fully-qualified-type`](#javafully-qualified-type) | semgrep | warning | medium | `java-source` |
| [`java.god-class`](#javagod-class) | pmd | warning | medium | `java-design` |
| [`java.ignored-interrupt`](#javaignored-interrupt) | semgrep | warning | medium | `java-source` |
| [`java.ignored-return-value`](#javaignored-return-value) | pmd | warning | medium | `java-review` |
| [`java.jspecify-null-marked`](#javajspecify-null-marked) | builtin | error | high | `jspecify` |
| [`java.lock-without-finally`](#javalock-without-finally) | semgrep | warning | medium | `java-source` |
| [`java.log-format-arguments`](#javalog-format-arguments) | pmd | warning | high | `java` |
| [`java.loose-coupling`](#javaloose-coupling) | pmd | warning | high | `java-design` |
| [`java.optional-misuse`](#javaoptional-misuse) | semgrep | warning | medium | `java-source` |
| [`java.preserve-stack-trace`](#javapreserve-stack-trace) | pmd | warning | medium | `java` |
| [`java.print-stack-trace`](#javaprint-stack-trace) | semgrep | error | high | `java-source` |
| [`java.resource-not-closed`](#javaresource-not-closed) | pmd | warning | medium | `java` |
| [`java.return-from-finally`](#javareturn-from-finally) | pmd | warning | high | `java` |
| [`java.return-null-collection`](#javareturn-null-collection) | pmd | warning | high | `java` |
| [`java.stdout-usage`](#javastdout-usage) | semgrep | error | high | `java-source` |
| [`java.string-reference-equality`](#javastring-reference-equality) | pmd | error | high | `java` |
| [`java.throw-from-finally`](#javathrow-from-finally) | pmd | warning | high | `java` |
| [`java.unnecessary-import`](#javaunnecessary-import) | pmd | warning | high | `java` |
| [`java.unused-local-variable`](#javaunused-local-variable) | pmd | warning | high | `java` |
| [`java.unused-private-field`](#javaunused-private-field) | pmd | warning | high | `java` |
| [`java.unused-private-method`](#javaunused-private-method) | pmd | warning | high | `java` |
| [`java.wildcard-import`](#javawildcard-import) | semgrep | warning | high | `java-source` |
| [`javascript.no-async-promise-executor`](#javascriptno-async-promise-executor) | eslint | warning | high | `javascript` |
| [`javascript.no-constant-binary-expression`](#javascriptno-constant-binary-expression) | eslint | warning | high | `javascript` |
| [`javascript.no-eval`](#javascriptno-eval) | eslint | warning | high | `javascript` |
| [`javascript.no-implied-eval`](#javascriptno-implied-eval) | eslint | warning | high | `javascript` |
| [`javascript.no-new-func`](#javascriptno-new-func) | eslint | warning | high | `javascript` |
| [`javascript.no-script-url`](#javascriptno-script-url) | eslint | warning | high | `javascript` |
| [`javascript.no-self-compare`](#javascriptno-self-compare) | eslint | warning | high | `javascript` |
| [`javascript.no-unsafe-finally`](#javascriptno-unsafe-finally) | eslint | warning | high | `javascript` |
| [`javascript.no-unsafe-negation`](#javascriptno-unsafe-negation) | eslint | warning | high | `javascript` |
| [`javascript.no-unsafe-optional-chaining`](#javascriptno-unsafe-optional-chaining) | eslint | warning | high | `javascript` |
| [`javascript.use-isnan`](#javascriptuse-isnan) | eslint | warning | high | `javascript` |
| [`javascript.valid-typeof`](#javascriptvalid-typeof) | eslint | warning | high | `javascript` |
| [`jsp.scripting-element`](#jspscripting-element) | semgrep | warning | high | `jsp` |
| [`jsp.unescaped-output`](#jspunescaped-output) | semgrep | warning | medium | `jsp` |
| [`kotlin.date-pattern-typo`](#kotlindate-pattern-typo) | semgrep | error | high | `kotlin` |
| [`kotlin.field-injection`](#kotlinfield-injection) | semgrep | error | high | `kotlin` |
| [`kotlin.fully-qualified-type`](#kotlinfully-qualified-type) | semgrep | warning | medium | `kotlin` |
| [`kotlin.global-scope`](#kotlinglobal-scope) | semgrep | warning | high | `kotlin` |
| [`kotlin.jpa-data-class-entity`](#kotlinjpa-data-class-entity) | semgrep | warning | high | `kotlin` |
| [`kotlin.missing-jpa-plugin`](#kotlinmissing-jpa-plugin) | gradle | error | high | `kotlin` |
| [`kotlin.missing-spring-plugin`](#kotlinmissing-spring-plugin) | gradle | error | high | `kotlin` |
| [`kotlin.not-null-assertion`](#kotlinnot-null-assertion) | semgrep | warning | high | `kotlin-review` |
| [`kotlin.print-stack-trace`](#kotlinprint-stack-trace) | semgrep | error | high | `kotlin` |
| [`kotlin.private-transactional`](#kotlinprivate-transactional) | semgrep | warning | high | `kotlin` |
| [`kotlin.run-blocking-in-suspend`](#kotlinrun-blocking-in-suspend) | semgrep | warning | high | `kotlin` |
| [`kotlin.run-catching-cancellation`](#kotlinrun-catching-cancellation) | semgrep | warning | medium | `kotlin` |
| [`kotlin.sql-injection`](#kotlinsql-injection) | semgrep | error | medium | `kotlin` |
| [`kotlin.static-member-injection`](#kotlinstatic-member-injection) | builtin | error | high | `kotlin` |
| [`kotlin.stdout-usage`](#kotlinstdout-usage) | semgrep | error | high | `kotlin` |
| [`kotlin.swallowed-cancellation`](#kotlinswallowed-cancellation) | semgrep | warning | medium | `kotlin` |
| [`kotlin.thread-sleep-in-suspend`](#kotlinthread-sleep-in-suspend) | semgrep | warning | high | `kotlin` |
| [`kotlin.transactional-suspend`](#kotlintransactional-suspend) | semgrep | warning | high | `kotlin` |
| [`kotlin.unmanaged-coroutine-scope`](#kotlinunmanaged-coroutine-scope) | semgrep | warning | medium | `kotlin` |
| [`rust.clippy`](#rustclippy) | clippy | warning | high | `rust` |
| [`rust.rustc-lint`](#rustrustc-lint) | clippy | warning | high | `rust` |
| [`security.cors-wildcard`](#securitycors-wildcard) | semgrep | error | medium | `security` |
| [`security.csrf-disabled`](#securitycsrf-disabled) | semgrep | warning | medium | `security` |
| [`security.disabled-tls-verification`](#securitydisabled-tls-verification) | semgrep | error | medium | `security` |
| [`security.jackson-default-typing`](#securityjackson-default-typing) | semgrep | warning | medium | `security` |
| [`security.sql-injection`](#securitysql-injection) | semgrep | error | medium | `security` |
| [`security.weak-crypto`](#securityweak-crypto) | semgrep | warning | medium | `security` |
| [`security.xxe`](#securityxxe) | semgrep | warning | medium | `security` |
| [`spring.async-self-invocation`](#springasync-self-invocation) | builtin | warning | high | `spring-boot` |
| [`spring.cache-self-invocation`](#springcache-self-invocation) | builtin | warning | high | `spring-boot` |
| [`spring.checked-exception-rollback`](#springchecked-exception-rollback) | semgrep | warning | medium | `spring-rollback` |
| [`spring.field-injection`](#springfield-injection) | semgrep | error | high | `spring-boot` |
| [`spring.http-client-without-timeout`](#springhttp-client-without-timeout) | semgrep | warning | medium | `spring-boot` |
| [`spring.jpa-eager-fetch`](#springjpa-eager-fetch) | semgrep | warning | high | `spring-boot` |
| [`spring.jpa-entity-request-body`](#springjpa-entity-request-body) | builtin | warning | high | `spring-boot` |
| [`spring.jpa-entity-response`](#springjpa-entity-response) | builtin | warning | medium | `jpa-web` |
| [`spring.jpa-lombok-entity`](#springjpa-lombok-entity) | semgrep | warning | high | `spring-boot` |
| [`spring.open-in-view-enabled`](#springopen-in-view-enabled) | builtin | warning | medium | `spring-boot` |
| [`spring.private-transactional`](#springprivate-transactional) | semgrep | warning | high | `spring-boot` |
| [`spring.redundant-autowired-constructor`](#springredundant-autowired-constructor) | semgrep | warning | high | `spring-boot` |
| [`spring.request-mapping-without-method`](#springrequest-mapping-without-method) | semgrep | warning | high | `spring-boot` |
| [`spring.retry-self-invocation`](#springretry-self-invocation) | builtin | warning | high | `spring-boot` |
| [`spring.shared-simple-date-format`](#springshared-simple-date-format) | semgrep | warning | medium | `spring-boot` |
| [`spring.singleton-request-state`](#springsingleton-request-state) | semgrep | warning | medium | `spring-boot` |
| [`spring.static-member-injection`](#springstatic-member-injection) | semgrep | error | high | `spring-boot` |
| [`spring.transactional-in-controller`](#springtransactional-in-controller) | semgrep | warning | high | `spring-boot` |
| [`spring.transactional-self-invocation`](#springtransactional-self-invocation) | builtin | warning | high | `spring-boot` |
| [`thymeleaf.restricted-bean-access`](#thymeleafrestricted-bean-access) | semgrep | error | high | `thymeleaf` |
| [`thymeleaf.unescaped-output`](#thymeleafunescaped-output) | semgrep | warning | medium | `thymeleaf-output-review` |
| [`typescript.no-floating-promises`](#typescriptno-floating-promises) | eslint | warning | high | `typescript-review` |

## architecture.adapter-isolation

An inbound adapter must not depend on an outbound adapter.

- Reports: A class in a package with an `inbound` segment that depends on a class in a package with an `outbound` segment (hexagonal profile). One diagnostic per depending class, with the number of dependencies and up to three examples.
- Fix: Route the dependency through an application port.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.<id>.options.from and rules.<id>.options.forbidden take package name segments, which replace the defaults listed under Reports.

Bad:

```java
package example.api.inbound;

class BookController {
    private final example.api.outbound.BookJpaAdapter adapter;
}
```

Good:

```java
package example.api.inbound;

class BookController {
    private final example.application.BookCatalog catalog; // an application port
}
```

## architecture.application-independence

Application code must not depend on adapters or infrastructure.

- Reports: A class in an `application` package that depends on a class in an `adapter` or `infrastructure` package (hexagonal profile).
- Fix: Depend on a port implemented by the adapter.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.<id>.options.from and rules.<id>.options.forbidden take package name segments, which replace the defaults listed under Reports.

Bad:

```java
package example.application;

class Orchestrator {
    Object adapter = new example.adapter.BookAdapter();
}
```

Good:

```java
package example.application;

class Orchestrator {
    private final BookStore store; // a port interface in application, implemented by the adapter
}
```

## architecture.controller-repository

A controller must not depend directly on a repository.

- Reports: A class in a `controller` package that depends on a class in a `repository` package (layered profile).
- Fix: Expose the operation through a service.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.<id>.options.from and rules.<id>.options.forbidden take package name segments, which replace the defaults listed under Reports.

Bad:

```java
package example.controller;

class BookController {
    private final example.repository.BookRepository books;
}
```

Good:

```java
package example.controller;

class BookController {
    private final example.service.BookService books;
}
```

## architecture.domain-independence

Domain code must remain independent of application and infrastructure.

- Reports: A class in a `domain` package that depends on a class in an `application`, `adapter` or `infrastructure` package (hexagonal profile).
- Fix: Put the dependency behind a domain abstraction.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.<id>.options.from and rules.<id>.options.forbidden take package name segments, which replace the defaults listed under Reports.

Bad:

```java
package example.domain;

record Book(String title) {
    Class<?> store() { return example.adapter.BookAdapter.class; }
}
```

Good:

```java
package example.domain;

record Book(String title) {}
```

## architecture.framework-independence

Domain and application code must not reference the Spring framework.

- Reports: A class in a `domain` or `application` package that references an `org.springframework` type, including annotations (hexagonal profile).
- Fix: Apply framework annotations and integrations in an adapter.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.<id>.options.from and rules.<id>.options.forbidden take package name segments, which replace the defaults listed under Reports.

Bad:

```java
package example.domain;

@org.springframework.stereotype.Component
class PriceCalculator {}
```

Good:

```java
package example.domain;

class PriceCalculator {} // registered as a bean in a configuration class outside domain
```

## architecture.library-framework-independence

A Java library must not reference the Spring framework.

- Reports: Any class of a module with the java-library profile that references an `org.springframework` type.
- Fix: Keep the framework integration in the consuming application.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.<id>.options.from and rules.<id>.options.forbidden take package name segments, which replace the defaults listed under Reports.

Bad:

```java
package example.common;

@org.springframework.stereotype.Component
public class Managed {}
```

Good:

```java
package example.common;

public class Managed {} // the application module declares the bean
```

## architecture.module-dependency

A forbidden Gradle project dependency is declared.

- Reports: A Gradle project dependency listed under module_dependencies.forbid in .dornguard.yml, such as `:api -> :admin`. Reported on the build file, once per forbidden pair.
- Fix: Remove the project dependency or depend on a shared module.
- Suppression: Do not suppress; change module_dependencies.forbid in .dornguard.yml if the dependency is now allowed.
- Configuration: module_dependencies.forbid: [{ from: ":module", to: ":other" }]

Bad:

```java
// api/build.gradle.kts with module_dependencies.forbid: [{ from: ":api", to: ":admin" }]
dependencies {
    implementation(project(":admin"))
}
```

Good:

```java
dependencies {
    implementation(project(":common")) // move the shared code to a module both may use
}
```

## architecture.package-cycle

Packages must not depend on each other in a cycle.

- Reports: Each package edge of a dependency cycle between packages in one module, with the number of class dependencies behind it (package-cycles profile). Dependencies on packages outside the cycle are not reported.
- Fix: Break the cycle by moving shared types to a lower-level package or inverting one dependency.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.architecture.package-cycle.options: { from: [example.service], cycles: true } considers only matching packages.

Bad:

```java
// example.order uses example.billing, and example.billing uses example.order
package example.billing;

class Invoice { example.order.Order order; }
```

Good:

```java
// example.billing depends on an interface it owns; example.order implements it
package example.billing;

class Invoice { Billable item; }
```

## architecture.repository-independence

Repositories must remain independent of service and controller layers.

- Reports: A class in a `repository` package that depends on a class in a `service` or `controller` package (layered profile).
- Fix: Move orchestration into a service.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.<id>.options.from and rules.<id>.options.forbidden take package name segments, which replace the defaults listed under Reports.

Bad:

```java
package example.repository;

final class AdminRepository {
    Class<?> upward = example.service.AdminService.class;
}
```

Good:

```java
package example.repository;

final class AdminRepository {} // the service calls the repository, not the other way
```

## architecture.service-independence

A service must not depend on the controller layer.

- Reports: A class in a `service` package that depends on a class in a `controller` package, such as a request or response type declared there (layered profile).
- Fix: Move web types such as request and response objects to the controller, and pass the service plain values or domain types.
- Suppression: Only for a dependency the team has accepted as a documented exception, such as generated code; otherwise move the code so the dependency goes the allowed way.
- Configuration: rules.<id>.options.from and rules.<id>.options.forbidden take package name segments, which replace the defaults listed under Reports.

Bad:

```java
package example.service;

class OrderService {
    void place(example.controller.OrderRequest request) {}
}
```

Good:

```java
package example.service;

class OrderService {
    void place(String customerId, int quantity) {} // the controller maps its request type
}
```

## flyway.changed-versioned-migration

An existing Flyway versioned migration changed or was removed.

- Reports: A modification, deletion or rename of an existing versioned Flyway migration such as `db/migration/V3__add_index.sql`, compared with --base or HEAD.
- Not reported: Newly added versions and repeatable `R__` migrations.
- Fix: Preserve the existing migration and introduce a new versioned migration for the change.
- Suppression: Only when the migration has never run in any shared database, for example it was added earlier in the same unmerged branch.

Bad:

```sql
-- db/migration/V3__add_index.sql, already applied, edited in place
CREATE INDEX idx_book_title ON book (title, author);
```

Good:

```sql
-- db/migration/V4__extend_book_index.sql, a new version
DROP INDEX idx_book_title;
CREATE INDEX idx_book_title ON book (title, author);
```

## format.blank-lines

This source has too many consecutive blank lines.

- Reports: More consecutive blank lines than format.max_blank_lines (1) in Java, Kotlin and JavaScript sources. Test sources are skipped unless skip_java_tests is false.
- Not reported: Blank lines inside string literals, text blocks and template literals.
- Fix: Remove excess blank lines.
- Suppression: Only for files whose layout is owned elsewhere, such as generated or vendored files; prefer format.exclude or an ignore entry with a path for those.
- Configuration: format.max_blank_lines (1), format.overrides[].max_blank_lines

Bad:

```java
int a = 1;


int b = 2;
```

Good:

```java
int a = 1;

int b = 2;
```

## format.control-spacing

Use one space before a control statement's opening parenthesis.

- Reports: A control keyword (if, for, while, switch, catch, synchronized) not followed by exactly one space before its opening parenthesis.
- Fix: Insert exactly one space between the control keyword and its opening parenthesis.
- Suppression: Only for files whose layout is owned elsewhere, such as generated or vendored files; prefer format.exclude or an ignore entry with a path for those.
- Configuration: format.control_spacing (space), format.overrides[].control_spacing

Bad:

```java
if(ready) {
    run();
}
```

Good:

```java
if (ready) {
    run();
}
```

## format.final-newline

End nonempty files with a newline.

- Reports: A nonempty source file that does not end with a newline.
- Fix: Add a line break after the last line.
- Suppression: Only for files whose layout is owned elsewhere, such as generated or vendored files; prefer format.exclude or an ignore entry with a path for those.

Bad:

```java
class A {}<end of file>
```

Good:

```java
class A {}
<end of file>
```

## format.indentation

Use spaces and the configured indentation for this language.

- Reports: When enabled (off by default), indentation with tabs or a width other than the configured one at block boundaries and for direct block members. Continuation lines may align freely.
- Fix: Indent each block level by the configured number of spaces.
- Suppression: Only for files whose layout is owned elsewhere, such as generated or vendored files; prefer format.exclude or an ignore entry with a path for those.
- Configuration: Off by default; enable with rules.format.indentation.enabled: true. format.indent_style (spaces), format.java_indent (4), format.kotlin_indent (4), format.javascript_indent (2), format.overrides[].indent_style and indent_size

Bad:

```java
void run() {
  start();
}
```

Good:

```java
void run() {
    start();
}
```

## format.line-ending

This file uses a different line ending than the configured format.

- Reports: A line ending other than the configured one (LF by default) anywhere in the file.
- Fix: Save the file with the configured line ending.
- Suppression: Only for files whose layout is owned elsewhere, such as generated or vendored files; prefer format.exclude or an ignore entry with a path for those.
- Configuration: format.line_ending (lf), format.overrides[].line_ending

Bad:

```java
class A {}\r\n
```

Good:

```java
class A {}\n
```

## format.trailing-whitespace

Remove trailing spaces and tabs.

- Reports: Spaces or tabs at the end of a line, outside string literals and text blocks.
- Fix: Delete the whitespace at the end of this line.
- Suppression: Only for files whose layout is owned elsewhere, such as generated or vendored files; prefer format.exclude or an ignore entry with a path for those.

Bad:

```java
int a = 1;··
```

Good:

```java
int a = 1;
```

## go.vet

go vet reported a suspicious construct.

- Reports: Every finding of the default go vet analyzers for the checked packages. The analyzer is the diagnostic's subject, such as printf, assign or copylocks.
- Not reported: Files under vendor and testdata directories.
- Fix: Fix the reported construct; go vet findings are almost always bugs.
- Suppression: Almost never; go vet findings are bugs. For a deliberate case, explain it in the dornguard-ignore reason.

Bad:

```go
fmt.Printf("%d\n", name) // name is a string
```

Good:

```go
fmt.Printf("%s\n", name)
```

## html.inline-event-handler

This HTML or Thymeleaf attribute contains an inline event handler.

- Reports: Inline event attributes such as onclick and onerror, and the Thymeleaf th:on* and data-th-on* forms, in HTML and templates.
- Fix: Register the event handler from a JavaScript file with addEventListener.
- Suppression: Only for pages that are never served with a Content Security Policy that blocks inline handlers, and say so in the reason.

Bad:

```html
<button onclick="save()">Save</button>
```

Good:

```html
<button id="save">Save</button>
<script src="/js/save.js"></script> <!-- addEventListener('click', save) -->
```

## html.inline-script

This HTML script contains inline JavaScript.

- Reports: An executable inline <script> block in HTML and templates.
- Not reported: Scripts loaded through src or th:src, and data blocks such as application/ld+json.
- Fix: Move the JavaScript to a file and load it with src.
- Suppression: Only for a script that carries a CSP nonce or hash the server sets, and say which in the reason.

Bad:

```html
<script>
  initPage();
</script>
```

Good:

```html
<script src="/js/page.js"></script>
```

## java.assert-throws-nested-call

A call that prepares an argument can throw before JUnit reaches the operation under test.

- Reports: A JUnit assertThrows or assertThrowsExactly lambda that passes the result of another method call directly to the operation under test, so preparing the argument may throw first and satisfy the assertion (java-review profile). Test sources are checked.
- Fix: Prepare the argument before assertThrows and keep the operation under test in the lambda.
- Suppression: When the nested call cannot throw the asserted exception, such as a constant factory; say why in the reason.

Bad:

```java
assertThrows(IllegalArgumentException.class,
    () -> parser.parse(loadFixture("bad.json")));
```

Good:

```java
String input = loadFixture("bad.json");
assertThrows(IllegalArgumentException.class, () -> parser.parse(input));
```

## java.bigdecimal-from-floating-point-literal

Constructing BigDecimal from a floating-point literal can introduce a rounding error.

- Reports: A decimal literal passed to the BigDecimal constructor, as reported by PMD.
- Fix: Use a decimal string or BigDecimal.valueOf for a double literal.
- Suppression: Do not suppress; the constructor keeps the binary rounding error of the literal.

Bad:

```java
BigDecimal price = new BigDecimal(0.1);
```

Good:

```java
BigDecimal price = new BigDecimal("0.1"); // or BigDecimal.valueOf(0.1)
```

## java.broken-null-check

This null check can still evaluate an operation on a null reference.

- Reports: A null check that still dereferences the value when it is null, usually && and || swapped, as reported by PMD.
- Fix: Use a short-circuit condition that accesses the value only when it is non-null.
- Suppression: Rarely; confirm the condition by evaluating it with a null value before suppressing.

Bad:

```java
if (name != null || name.isEmpty()) {
    return;
}
```

Good:

```java
if (name == null || name.isEmpty()) {
    return;
}
```

## java.cognitive-complexity

This method's cognitive complexity is 15 or more, which makes it hard to read and test.

- Reports: A method whose PMD cognitive complexity is 15 or more (java-design profile). Nesting, breaks in control flow and boolean operator sequences add to it.
- Fix: Extract nested conditions and loops into well-named methods, return early, or replace branching on a type with polymorphism.
- Suppression: When the method mirrors an external specification step by step, such as a parser state machine, and splitting it would hide the mapping; say so in the reason.

Bad:

```java
int score(int[] values, boolean strict) {
    for (int value : values) {
        if (value > 0) {
            if (strict) {
                for (...) { if (... && ... || ...) { ... } else if (...) { ... } }
            } else if (...) { while (...) { ... } }
        } else { switch (value) { ... } }
    }
}
```

Good:

```java
int score(int[] values, boolean strict) {
    int total = 0;
    for (int value : values) {
        total += value > 0 ? positiveScore(value, strict) : negativeScore(value);
    }
    return total;
}
```

## java.date-pattern-typo

A date pattern uses the week-based year (YYYY) or the day of the year (DD) by mistake.

- Reports: A DateTimeFormatter, SimpleDateFormat or Commons Lang FastDateFormat/DateFormatUtils pattern literal that uses the week-based year YYYY without a week-of-year field w, or the day of the year D together with a month M.
- Not reported: Patterns built at run time and applyPattern calls.
- Fix: Use yyyy for the calendar year and dd for the day of the month. YYYY gives the wrong year for the last days of December, and DD after MM gives the day of the year, so both pass tests written for most dates. Keep YYYY only together with week-of-year fields such as ww.
- Suppression: Only for a week-date or ordinal-date format that the reader expects, and name that format in the reason.

Bad:

```java
DateTimeFormatter.ofPattern("YYYY-MM-DD")
```

Good:

```java
DateTimeFormatter.ofPattern("yyyy-MM-dd")
```

## java.double-checked-locking

Double-checked locking on a non-volatile field can publish a partly constructed object.

- Reports: Double-checked lazy initialization of a field that is not volatile, as reported by PMD.
- Fix: Declare the field volatile, or use a holder class or an enum for lazy initialization.
- Suppression: Do not suppress; make the field volatile or use a holder class.

Bad:

```java
private static Config instance;

static Config get() {
    if (instance == null) {
        synchronized (Config.class) {
            if (instance == null) instance = new Config();
        }
    }
    return instance;
}
```

Good:

```java
private static volatile Config instance; // or a holder class
```

## java.empty-catch

Handle exceptions explicitly.

- Reports: A catch block with no statements and no comment, as reported by PMD.
- Not reported: A catch block that contains a comment explaining why the exception is ignored.
- Fix: Recover, report the exception, or propagate it to a caller that can handle it.
- Suppression: Not needed; a comment in the block that says why the exception is ignored is enough.

Bad:

```java
try {
    Files.delete(temp);
} catch (IOException e) {
}
```

Good:

```java
try {
    Files.delete(temp);
} catch (IOException e) {
    // The temporary file is removed on the next start.
}
```

## java.equals-hashcode-contract

Overriding equals or hashCode alone can break hash-based collections.

- Reports: A class that overrides equals without hashCode, or hashCode without equals, as reported by PMD.
- Fix: Implement equals and hashCode together so equal objects have the same hash code.
- Suppression: Do not suppress; hash-based collections break when only one is overridden.

Bad:

```java
@Override
public boolean equals(Object other) { ... } // no hashCode
```

Good:

```java
@Override
public boolean equals(Object other) { ... }

@Override
public int hashCode() { return Objects.hash(id); }
```

## java.excessive-parameters

This method or constructor declares eight or more parameters.

- Reports: A method or constructor with eight or more parameters (java-design profile).
- Fix: Group related parameters into a value object or record, or split the method by responsibility.
- Suppression: For a framework-defined signature or a generated constructor, such as a JPA projection; say which in the reason.

Bad:

```java
void register(String name, String email, String phone, String street,
              String city, String zip, String country, boolean active) {}
```

Good:

```java
record Contact(String name, String email, String phone, Address address) {}

void register(Contact contact, boolean active) {}
```

## java.fully-qualified-type

This reference looks like a fully qualified type name.

- Reports: A package-qualified name shaped like a type, such as `java.util.List`, in Java code.
- Not reported: Import and package declarations, comments, string literals, and qualified expressions rooted in a visible local value.
- Fix: Use an import and the simple type name where it is unambiguous.
- Suppression: When two imported types share a simple name and the qualifier tells them apart; name both types in the reason.

Bad:

```java
java.util.List<String> names = new java.util.ArrayList<>();
```

Good:

```java
import java.util.ArrayList;
import java.util.List;

List<String> names = new ArrayList<>();
```

## java.god-class

This class is large, complex and uses many other classes' data, which suggests it has too many responsibilities.

- Reports: A class that PMD's GodClass rule finds large (high weighted method count), complex and dependent on other classes' data, with low cohesion (java-design profile).
- Fix: Split the class by responsibility and move behavior next to the data it uses.
- Suppression: For a facade that only delegates, such as a generated client; say so in the reason.

Bad:

```java
class OrderManager {
    // validation, pricing, persistence, mail and reporting in one class,
    // reading the fields of Customer, Invoice and Shipment directly
}
```

Good:

```java
class OrderValidator { ... }
class PriceCalculator { ... }
class OrderRepository { ... }
```

## java.ignored-interrupt

An InterruptedException is logged and consumed.

- Reports: An InterruptedException catch block whose only statement is a logging call, optionally followed by a bare return, so the interrupt status is lost.
- Not reported: Blocks that rethrow, restore the status with Thread.currentThread().interrupt() or call something other than a logger.
- Fix: Rethrow the exception or restore the interrupt status with Thread.currentThread().interrupt().
- Suppression: When the thread ends right after the catch and nothing reads its interrupt status; say so in the reason.

Bad:

```java
catch (InterruptedException e) {
    log.warn("Interrupted", e);
}
```

Good:

```java
catch (InterruptedException e) {
    Thread.currentThread().interrupt();
    log.warn("Interrupted", e);
}
```

## java.ignored-return-value

This call discards the result of a method whose return value should be used.

- Reports: A call whose result must be used, such as String.trim() or BigDecimal.add(), whose result the caller discards, as reported by PMD (java-review profile).
- Fix: Use the returned value in the next operation or remove the call if the result is unnecessary.
- Suppression: When the call is made only for an exception it may throw, such as a validating parse; say so in the reason.

Bad:

```java
name.trim();
total.add(tax);
```

Good:

```java
name = name.trim();
total = total.add(tax);
```

## java.jspecify-null-marked

Java types must be in an org.jspecify.annotations.NullMarked scope.

- Reports: A Java type outside an org.jspecify.annotations.NullMarked scope (jspecify profile). A package with no scope is reported once at its first type, with the type count and up to three names; types opted out with @NullUnmarked are reported one by one.
- Not reported: Types covered by @NullMarked on the type, an enclosing declaration, the package's package-info.java or the module's module-info.java.
- Fix: Apply JSpecify @NullMarked to the type or its package and remove any @NullUnmarked that overrides it.
- Suppression: For generated packages; add an ignore entry with the package path and say which generator in the reason.

Bad:

```java
// src/main/java/com/example/service/OrderService.java, no package-info.java
package com.example.service;

class OrderService {}
```

Good:

```java
// src/main/java/com/example/service/package-info.java
@NullMarked
package com.example.service;

import org.jspecify.annotations.NullMarked;
```

## java.lock-without-finally

A lock is acquired without a try/finally that releases it, so an exception keeps it held.

- Reports: lock() or lockInterruptibly() on a java.util.concurrent lock, including readLock() and writeLock(), that is not followed by or inside a try block with finally.
- Fix: Call lock() immediately before a try block and unlock() in its finally block. If the lock is released by another method on purpose, suppress the finding with a reason.
- Suppression: When another method releases the lock by design, such as a lock handed to a caller; name that method in the reason.

Bad:

```java
lock.lock();
update();
lock.unlock();
```

Good:

```java
lock.lock();
try {
    update();
} finally {
    lock.unlock();
}
```

## java.log-format-arguments

The number of {} placeholders in a log message does not match its arguments.

- Reports: A log call whose {} placeholders do not match the number of arguments, as reported by PMD.
- Not reported: A trailing exception argument, which needs no placeholder. Without the classpath, PMD cannot tell that a project or library exception is a Throwable, so a trailing parameter or local whose declared type name ends in Exception, Error or Throwable is also treated as the exception when the other arguments match the placeholders.
- Fix: Pass one argument for each {} placeholder. Pass an exception as the last argument without a placeholder so the logger records its stack trace.
- Suppression: Do not suppress; fix the message or the arguments.

Bad:

```java
log.info("user {} signed in from {}", user);
```

Good:

```java
log.info("user {} signed in from {}", user, address);
```

## java.loose-coupling

A field, parameter or return type names a collection implementation such as ArrayList or HashMap instead of its interface.

- Reports: A field, parameter or return type declared as a collection implementation such as ArrayList, HashMap or HashSet instead of its interface (java-design profile).
- Fix: Declare the type as List, Set, Map or another interface, and choose the implementation only where the object is created.
- Suppression: When callers rely on the implementation's guarantees, such as LinkedHashMap ordering or ArrayDeque operations missing from the interface; say which in the reason.

Bad:

```java
HashMap<String, Integer> counts() { return new HashMap<>(); }
```

Good:

```java
Map<String, Integer> counts() { return new HashMap<>(); }
```

## java.optional-misuse

Optional is used as a field or parameter type, or a method that returns Optional returns null.

- Reports: An Optional field, an Optional method or constructor parameter, or `return null` in a method that returns Optional.
- Not reported: Optional local variables and return types, and `return null` inside a lambda or anonymous class.
- Fix: Return Optional.empty() instead of null. Store a nullable field and expose an Optional getter, and take the plain type as a parameter with an overload for the absent case.
- Suppression: When a framework requires the Optional field or parameter, such as a serialization contract you do not own; name it in the reason.

Bad:

```java
private Optional<String> nickname;

Optional<User> find(String id) {
    return null;
}
```

Good:

```java
private @Nullable String nickname;

Optional<String> nickname() { return Optional.ofNullable(nickname); }

Optional<User> find(String id) {
    return Optional.empty();
}
```

## java.preserve-stack-trace

This exception drops the original cause.

- Reports: A catch block that throws a new exception without passing the caught one as its cause, as reported by PMD.
- Not reported: Rethrowing the caught exception, also after an instanceof check, throwing the exception a caught wrapper carries, such as `throw e.original()` or `throw (RuntimeException) e.getCause()`, and, with allow_logged_exceptions, a caught exception that is logged first.
- Fix: Pass the caught exception as the cause.
- Suppression: When the cause must not leave a security boundary, such as an error returned to a client; say so in the reason.
- Configuration: allow_logged_exceptions (false)

Bad:

```java
catch (NumberFormatException e) {
    throw new IllegalStateException(e.getMessage());
}
```

Good:

```java
catch (NumberFormatException e) {
    throw new IllegalStateException("Invalid port", e);
}
```

## java.print-stack-trace

Avoid printing stack traces to standard error.

- Reports: A call to printStackTrace(), which writes to standard error instead of the logger.
- Fix: Log the exception with the project logger and pass it as the last argument.
- Suppression: Only in a command-line tool without a logger; say so in the reason.

Bad:

```java
catch (IOException e) {
    e.printStackTrace();
}
```

Good:

```java
catch (IOException e) {
    log.error("Could not read {}", path, e);
}
```

## java.resource-not-closed

The code may leave a Java resource open.

- Reports: A resource such as a stream, reader or JDBC connection that the code opens and may leave open on some path, as reported by PMD's CloseResource.
- Not reported: Resources returned to the caller, stored in a field the class closes, opened by factories listed in java_managed_resource_factories, or owned by types in java_resource_owners. In a Spring @Bean method, a resource the method only passes to a constructor or a builder chain in a local initializer or the return statement, since the bean being built owns it; the same holds in a private helper that only @Bean methods call.
- Fix: Use try-with-resources or close the resource according to its ownership.
- Suppression: When ownership passes elsewhere in a way PMD cannot see, such as a framework that closes it; name the owner in the reason, or list it in java_resource_owners.
- Configuration: java_target, java_resource_owners, java_managed_resource_factories

Bad:

```java
InputStream in = Files.newInputStream(file);
return in.readAllBytes();
```

Good:

```java
try (InputStream in = Files.newInputStream(file)) {
    return in.readAllBytes();
}
```

## java.return-from-finally

Returning from finally can suppress an exception or replace the original return value.

- Reports: A return statement inside a finally block, as reported by PMD.
- Fix: Keep cleanup in finally and return outside the finally block.
- Suppression: Do not suppress; it hides exceptions from the try block.

Bad:

```java
try {
    return compute();
} finally {
    return fallback;
}
```

Good:

```java
try {
    return compute();
} finally {
    cleanup();
}
```

## java.return-null-collection

A method that returns a collection or array returns null.

- Reports: `return null` in a method that returns a collection, map or array, as reported by PMD.
- Not reported: A method whose return type declares null with @Nullable on its outermost type, such as `@Nullable List<String>` or `String @Nullable []`. `@Nullable String[]` marks only the elements nullable and stays reported.
- Fix: Return an empty collection or array, such as List.of() or new String[0], so callers need no null check.
- Suppression: When null means something different from empty to existing callers, such as 'not loaded'; say so in the reason, and prefer Optional for new code.

Bad:

```java
List<String> names() {
    if (!loaded) return null;
    return names;
}
```

Good:

```java
List<String> names() {
    if (!loaded) return List.of();
    return names;
}
```

## java.stdout-usage

Avoid printing directly to standard output.

- Reports: System.out.println calls.
- Not reported: Calls inside a main entry point, because a command-line tool writes to standard output by design. Helpers that main calls are still reported.
- Fix: Use the project logger.
- Suppression: When standard output is the program's interface, such as a tool whose output another script reads; say so in the reason.

Bad:

```java
System.out.println("progress " + count);
```

Good:

```java
log.info("progress {}", count);
```

## java.string-reference-equality

Strings are compared with == or != instead of equals.

- Reports: Strings compared with == or !=, as reported by PMD.
- Not reported: Comparisons with null, and an identity check of a local initialized by a call that takes the other operand, such as `String encoded = encode(text); if (encoded == text)`, which asks whether the call returned the same instance.
- Fix: Use equals, or Objects.equals when either side can be null. == compares object identity and fails for strings built at run time.
- Suppression: Only for interned constants compared on a measured hot path; say so in the reason.

Bad:

```java
if (role == "admin") { ... }
```

Good:

```java
if ("admin".equals(role)) { ... }
```

## java.throw-from-finally

Throwing from finally replaces the exception or return value of the try block.

- Reports: A throw statement inside a finally block, as reported by PMD.
- Fix: Keep cleanup in finally and throw outside it. If cleanup can fail, catch that failure and add it to the original exception with addSuppressed.
- Suppression: Do not suppress; it replaces the exception or return value of the try block.

Bad:

```java
finally {
    if (!closed) throw new IllegalStateException("not closed");
}
```

Good:

```java
finally {
    try { close(); }
    catch (IOException e) { failure.addSuppressed(e); }
}
```

## java.unnecessary-import

This Java import is unnecessary.

- Reports: Unused, duplicate or implicit imports (java.lang, same package), as reported by PMD. Unused wildcard imports are reported when the module's compiled classpath is available.
- Not reported: An import whose type the file also writes in full; java.fully-qualified-type reports that once and asks for the simple name.
- Fix: Remove the unused, duplicated, or implicit import.
- Suppression: When PMD lacks the classpath to see a use, such as a type used only in Javadoc; say so in the reason, or run with --resolve-imports.

Bad:

```java
import java.util.List; // never used
import java.lang.String;
```

Good:

```java
// both imports removed
```

## java.unused-local-variable

This local variable is assigned but never used.

- Reports: A local variable that is declared or assigned but never read, as reported by PMD.
- Not reported: Variables whose name starts with ignored or unused.
- Fix: Remove the variable, or use its value where it was meant to be used.
- Suppression: Not needed; rename a deliberately unused variable to ignored or unused, for example in a catch or try-with-resources.

Bad:

```java
String copy = label;
return format();
```

Good:

```java
return format();
```

## java.unused-private-field

This private field is never read.

- Reports: A private field that is never read, as reported by PMD.
- Not reported: serialVersionUID and fields read by Lombok-generated accessors.
- Fix: Remove the field, or use it where its value was meant to be read.
- Suppression: When reflection reads the field, such as a serialization framework PMD does not know; name it in the reason.

Bad:

```java
private String lastUser; // assigned, never read
```

Good:

```java
// field removed, or read where its value was meant to be used
```

## java.unused-private-method

This private method is never called.

- Reports: A private method that is never called, as reported by PMD.
- Fix: Remove the method, or call it where it was meant to run.
- Suppression: When a framework calls it reflectively, such as a JUnit @MethodSource factory or a serialization hook; name the caller in the reason.

Bad:

```java
private void leftover() {}
```

Good:

```java
// method removed
```

## java.wildcard-import

A wildcard import hides which types the file uses and can make a new type in the package collide with another import.

- Reports: A non-static or static wildcard import (`import ...*;`).
- Not reported: An unused wildcard import, which java.unnecessary-import reports instead.
- Fix: Replace the wildcard with an import for each type the file uses.
- Suppression: When a generated or vendored file uses it; prefer an ignore entry with the file's path.

Bad:

```java
import java.util.*;
```

Good:

```java
import java.util.ArrayList;
import java.util.List;
```

## javascript.no-async-promise-executor

An async Promise executor can leave the outer Promise pending if it throws.

- Reports: An async function passed as the executor of new Promise (ESLint no-async-promise-executor).
- Fix: Use an async function directly or keep the Promise executor synchronous.
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
new Promise(async (resolve) => {
  resolve(await load());
});
```

Good:

```javascript
const value = await load(); // or: load().then(resolve, reject)
```

## javascript.no-constant-binary-expression

A comparison or logical expression has a result that does not depend on its operands.

- Reports: A comparison or logical expression whose result does not depend on its operands, such as `x === []` or `a + b ?? c` (ESLint no-constant-binary-expression).
- Fix: Check operator precedence and compare with the intended value. A new object or array never equals another value, and a ?? b has no effect when a cannot be null.
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
if (items === []) { showEmpty(); }
```

Good:

```javascript
if (items.length === 0) { showEmpty(); }
```

## javascript.no-eval

Avoid evaluating strings as JavaScript with eval.

- Reports: Calls to eval (ESLint no-eval).
- Fix: Use a direct function call or a data parser.
- Suppression: Only in build or development tooling that never runs on untrusted input; say so in the reason.

Bad:

```javascript
const config = eval("(" + text + ")");
```

Good:

```javascript
const config = JSON.parse(text);
```

## javascript.no-implied-eval

Avoid sending JavaScript strings to APIs that evaluate them.

- Reports: A string passed as code to setTimeout, setInterval or execScript (ESLint no-implied-eval).
- Fix: Pass a callback function instead of a string.
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
setTimeout("refresh()", 1000);
```

Good:

```javascript
setTimeout(() => refresh(), 1000);
```

## javascript.no-new-func

Avoid creating functions from strings.

- Reports: new Function(...) or Function(...) creating a function from strings (ESLint no-new-func).
- Fix: Use a declared function or a function expression.
- Suppression: Only in tooling that compiles trusted templates; say so in the reason.

Bad:

```javascript
const add = new Function("a", "b", "return a + b");
```

Good:

```javascript
const add = (a, b) => a + b;
```

## javascript.no-script-url

A javascript: URL runs code like eval and is blocked by a strict Content Security Policy.

- Reports: A `javascript:` URL string (ESLint no-script-url).
- Fix: Attach an event listener, or use a regular URL.
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
link.href = "javascript:void(0)";
```

Good:

```javascript
link.addEventListener("click", (event) => event.preventDefault());
```

## javascript.no-self-compare

A value is compared with itself, which is always true or false except for NaN.

- Reports: A value compared with itself (ESLint no-self-compare).
- Fix: Compare with the intended value. Use Number.isNaN(value) to test for NaN.
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
if (value !== value) { ... } // NaN check
```

Good:

```javascript
if (Number.isNaN(value)) { ... }
```

## javascript.no-unsafe-finally

A return, throw, break or continue in finally replaces the outcome of the try block.

- Reports: return, throw, break or continue inside finally (ESLint no-unsafe-finally).
- Fix: Keep cleanup in finally and return or throw outside it.
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
try { return save(); } finally { return false; }
```

Good:

```javascript
try { return save(); } finally { release(); }
```

## javascript.no-unsafe-negation

The ! operator applies to the left operand of in or instanceof instead of the whole test.

- Reports: ! applied to the left operand of in or instanceof (ESLint no-unsafe-negation).
- Fix: Parenthesize the test, for example !(key in object) or !(value instanceof Type).
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
if (!key in settings) { ... }
```

Good:

```javascript
if (!(key in settings)) { ... }
```

## javascript.no-unsafe-optional-chaining

An optional chain that can be undefined is used where undefined throws a TypeError.

- Reports: An optional chain that can be undefined used where undefined throws, such as a call, spread or destructuring (ESLint no-unsafe-optional-chaining).
- Fix: Provide a fallback such as (obj?.items ?? []) before calling, spreading or destructuring the result.
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
const { name } = user?.profile;
```

Good:

```javascript
const { name } = user?.profile ?? {};
```

## javascript.use-isnan

A comparison with NaN is always false.

- Reports: A comparison with NaN (ESLint use-isnan).
- Fix: Use Number.isNaN(value). In a switch, test Number.isNaN before the switch.
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
if (total === NaN) { ... }
```

Good:

```javascript
if (Number.isNaN(total)) { ... }
```

## javascript.valid-typeof

typeof is compared with a string it never returns.

- Reports: typeof compared with a string it never returns (ESLint valid-typeof).
- Fix: Compare with one of "undefined", "object", "boolean", "number", "string", "function", "symbol" or "bigint".
- Suppression: Do not suppress; the code does not do what it appears to.

Bad:

```javascript
if (typeof callback === "fucntion") { ... }
```

Good:

```javascript
if (typeof callback === "function") { ... }
```

## jsp.scripting-element

This JSP contains embedded scripting code.

- Reports: JSP scripting elements: <% ... %>, <%= ... %>, <%! ... %> and their <jsp:scriptlet>, <jsp:expression> and <jsp:declaration> forms. Directives <%@ ... %> and comments are not reported.
- Fix: Move logic to Java code and use JSP EL or tag libraries for rendering.
- Suppression: Only in a legacy page scheduled for removal, with the ticket in the reason.

Bad:

```html
<%= request.getParameter("name") %>
```

Good:

```html
<c:out value="${param.name}"/>
```

## jsp.unescaped-output

This JSTL output explicitly disables XML escaping.

- Reports: A JSTL <c:out> tag with a literal escapeXml="false".
- Fix: Keep escaping enabled for text or verify that the HTML is sanitized before rendering.
- Suppression: When the value is HTML the server produced or sanitized; name the sanitizer in the reason.

Bad:

```html
<c:out value="${comment.body}" escapeXml="false"/>
```

Good:

```html
<c:out value="${comment.body}"/>
```

## kotlin.date-pattern-typo

A date pattern uses the week-based year (YYYY) or the day of the year (DD) by mistake.

- Reports: A DateTimeFormatter or SimpleDateFormat pattern literal in Kotlin with the week-based year YYYY and no week field w, or the day of the year D together with a month M.
- Not reported: String templates and applyPattern calls.
- Fix: Use yyyy for the calendar year and dd for the day of the month. YYYY gives the wrong year for the last days of December, and DD after MM gives the day of the year, so both pass tests written for most dates. Keep YYYY only together with week-of-year fields such as ww.
- Suppression: Only for a week-date or ordinal-date format the reader expects; name it in the reason.

Bad:

```kotlin
DateTimeFormatter.ofPattern("YYYY-MM-DD")
```

Good:

```kotlin
DateTimeFormatter.ofPattern("yyyy-MM-dd")
```

## kotlin.field-injection

Spring field injection hides a class's dependencies.

- Reports: A Kotlin lateinit var or var property annotated with @Autowired, @Inject or @Resource.
- Not reported: Constructor injection, including `@Autowired constructor`.
- Fix: Take the dependency as a primary constructor parameter, declared as private val, and remove the annotated lateinit var. A circular dependency needs a design change instead: extract the shared logic into a third bean.
- Suppression: In a test class that Spring's test framework injects; test sources are skipped by default.

Bad:

```kotlin
class Catalog {
    @Autowired
    lateinit var books: BookRepository
}
```

Good:

```kotlin
class Catalog(private val books: BookRepository)
```

## kotlin.fully-qualified-type

This reference looks like a fully qualified type name.

- Reports: A package-qualified name shaped like a type in Kotlin code.
- Not reported: Import and package declarations, comments and string literals.
- Fix: Use an import and the simple type name where it is unambiguous.
- Suppression: When two imported types share a simple name; use an import alias (`import a.B as AB`) instead where possible.

Bad:

```kotlin
val names = java.util.ArrayList<String>()
```

Good:

```kotlin
import java.util.ArrayList

val names = ArrayList<String>()
```

## kotlin.global-scope

GlobalScope launches work outside the caller's lifecycle.

- Reports: GlobalScope.launch or GlobalScope.async.
- Fix: Use coroutineScope or a CoroutineScope owned by the component that manages the work.
- Suppression: For work that must outlive every component, such as a process-wide shutdown hook; say so in the reason.

Bad:

```kotlin
GlobalScope.launch { refresh() }
```

Good:

```kotlin
scope.launch { refresh() } // a scope the owner cancels, or coroutineScope { } in a suspend function
```

## kotlin.jpa-data-class-entity

A Kotlin data class is used as a JPA entity.

- Reports: A Kotlin data class annotated with @Entity. Its generated equals, hashCode and toString read every property, including lazy associations.
- Fix: Use a regular class for the entity, with the kotlin-jpa and kotlin-spring (all-open) compiler plugins. Data class equals, hashCode and toString read every property, which loads lazy associations and changes the hash code when the generated id is assigned. Keep data classes for DTOs.
- Suppression: Do not suppress; keep data classes for DTOs.

Bad:

```kotlin
@Entity
data class Book(@Id val id: Long, val title: String)
```

Good:

```kotlin
@Entity
class Book(@Id val id: Long, var title: String) {
    override fun equals(other: Any?) = other is Book && other.id == id
    override fun hashCode() = id.hashCode()
}
```

## kotlin.missing-jpa-plugin

A Gradle module declares Kotlin JPA entities without the Kotlin JPA compiler plugin, so the entities have no no-arg constructor for JPA to instantiate them.

- Reports: A Gradle Kotlin module without the kotlin("plugin.jpa") or noarg plugin that declares an @Entity, @Embeddable or @MappedSuperclass class. Reported once per module at the first such class.
- Fix: Apply the Kotlin JPA plugin, for example plugins { kotlin("plugin.jpa") version "<kotlin version>" }, which adds a no-arg constructor to @Entity, @Embeddable and @MappedSuperclass classes.
- Suppression: Do not suppress; JPA cannot instantiate the entity without a no-arg constructor.

Bad:

```kotlin
plugins {
    kotlin("jvm") version "2.4.21"
}
```

Good:

```kotlin
plugins {
    kotlin("jvm") version "2.4.21"
    kotlin("plugin.jpa") version "2.4.21"
}
```

## kotlin.missing-spring-plugin

A Gradle module declares final Kotlin classes that Spring must proxy, such as @Configuration or @Transactional classes, without the Kotlin Spring compiler plugin.

- Reports: A Gradle Kotlin module without the kotlin("plugin.spring") or allopen plugin that has a final class or function annotated with @Configuration, @SpringBootApplication, @Transactional, @Async, a cache annotation, @Retryable or @Validated. Reported once per module at the first such declaration.
- Not reported: Declarations marked open or abstract, override functions (other than final override) in an open class, and interfaces.
- Fix: Apply the Kotlin Spring plugin, for example plugins { kotlin("plugin.spring") version "<kotlin version>" }, which opens Spring-annotated classes and their members. Declaring each class and function open also works.
- Suppression: Do not suppress; apply the plugin, or mark each proxied class and function open.

Bad:

```kotlin
plugins {
    kotlin("jvm") version "2.4.21"
}
// @Transactional class Catalog
```

Good:

```kotlin
plugins {
    kotlin("jvm") version "2.4.21"
    kotlin("plugin.spring") version "2.4.21"
}
```

## kotlin.not-null-assertion

The not-null assertion operator (!!) throws a NullPointerException with no context when the value is null.

- Reports: Each use of the not-null assertion operator !! (kotlin-review profile).
- Fix: Handle null explicitly with ?., ?: or a smart cast, or use requireNotNull or checkNotNull with a message that says why the value cannot be null.
- Suppression: When a framework guarantees the value right after the check, such as a test fixture; prefer requireNotNull with a message.

Bad:

```kotlin
val length = name!!.length
```

Good:

```kotlin
val length = requireNotNull(name) { "name is set by init()" }.length
```

## kotlin.print-stack-trace

Avoid printing stack traces to standard error.

- Reports: A call to printStackTrace() in Kotlin.
- Fix: Log the exception with the project logger and pass it as the last argument.
- Suppression: Only in a command-line tool without a logger; say so in the reason.

Bad:

```kotlin
catch (e: IOException) {
    e.printStackTrace()
}
```

Good:

```kotlin
catch (e: IOException) {
    log.error("Could not read {}", path, e)
}
```

## kotlin.private-transactional

Spring proxy mode cannot apply transaction advice to a private function.

- Reports: A private Kotlin function annotated with Spring's @Transactional, which a proxy cannot advise.
- Fix: Place the transaction boundary on a public function that callers reach through the Spring proxy.
- Suppression: Do not suppress; the annotation has no effect.

Bad:

```kotlin
@Transactional
private fun save() {}
```

Good:

```kotlin
@Transactional
fun save() {} // called through the bean, not from the same class
```

## kotlin.run-blocking-in-suspend

runBlocking blocks the thread inside a suspend function.

- Reports: kotlinx.coroutines.runBlocking inside a suspend function.
- Fix: Call the suspending operation directly or use coroutineScope for structured concurrency.
- Suppression: Do not suppress; it blocks the coroutine's thread.

Bad:

```kotlin
suspend fun load() = runBlocking { fetch() }
```

Good:

```kotlin
suspend fun load() = fetch()
```

## kotlin.run-catching-cancellation

runCatching can store cancellation from this suspending coroutine operation as a failure.

- Reports: A known suspending coroutine call, such as delay or withContext, or a direct call to a sibling no-argument suspend function, inside runCatching, which turns cancellation into a failure value.
- Not reported: runCatching { ... }.getOrThrow() chains.
- Fix: Let cancellation propagate or explicitly rethrow CancellationException.
- Suppression: When the block cannot be cancelled, such as inside NonCancellable; say so in the reason.

Bad:

```kotlin
val result = runCatching { delay(100); fetch() }
```

Good:

```kotlin
val result = try {
    Result.success(fetch())
} catch (e: CancellationException) {
    throw e
} catch (e: IOException) {
    Result.failure(e)
}
```

## kotlin.sql-injection

SQL text built by string interpolation, concatenation, or formatting can allow SQL injection.

- Reports: A JDBC, JPA or JdbcTemplate query method in Kotlin whose SQL string interpolates, concatenates or formats a value.
- Fix: Use bind parameters such as ? or :name, and pass the values separately.
- Suppression: When the interpolated value is a constant or comes from a fixed allow-list, such as a validated sort column; name the allow-list in the reason.

Bad:

```kotlin
jdbc.query("select * from book where title = '$title'", mapper)
```

Good:

```kotlin
jdbc.query("select * from book where title = ?", mapper, title)
```

## kotlin.static-member-injection

Spring does not inject members of a companion object or an object declaration, which compile to static members, so the value stays unset.

- Reports: Spring's @Value on a property or function, or @Autowired on a function, declared in a companion object or an object declaration, including use-site targets such as @field:Value.
- Not reported: @Autowired properties, which kotlin.field-injection reports, members of a class nested inside the object, and annotations named Value from other packages.
- Fix: Move the property into the bean class and take the value as a primary constructor parameter, such as `class Mailer(@Value("\${mail.host}") private val host: String)`, or bind it with @ConfigurationProperties and pass the bean where it is needed.
- Suppression: Do not suppress; Spring never sets the value.

Bad:

```kotlin
class Mailer {
    companion object {
        @Value("\${mail.host}")
        lateinit var host: String
    }
}
```

Good:

```kotlin
class Mailer(@Value("\${mail.host}") private val host: String)
```

## kotlin.stdout-usage

Avoid printing directly to standard output.

- Reports: println calls in Kotlin.
- Not reported: Calls inside a main entry point.
- Fix: Use the project logger.
- Suppression: When standard output is the program's interface; say so in the reason.

Bad:

```kotlin
println("progress $count")
```

Good:

```kotlin
log.info("progress {}", count)
```

## kotlin.swallowed-cancellation

This catch consumes a coroutine cancellation exception.

- Reports: A catch of CancellationException (or a coroutine cancellation subtype) that is empty, returns without a value or only logs.
- Fix: Rethrow the cancellation exception after any logging or cleanup.
- Suppression: Do not suppress; rethrow so structured concurrency can cancel.

Bad:

```kotlin
catch (e: CancellationException) {
    log.warn("cancelled", e)
}
```

Good:

```kotlin
catch (e: CancellationException) {
    log.warn("cancelled", e)
    throw e
}
```

## kotlin.thread-sleep-in-suspend

Thread.sleep blocks the thread inside a suspend function.

- Reports: Thread.sleep inside a suspend function.
- Fix: Use kotlinx.coroutines.delay for a suspending wait.
- Suppression: Do not suppress; it blocks the thread the coroutine runs on.

Bad:

```kotlin
suspend fun poll() { Thread.sleep(1000) }
```

Good:

```kotlin
suspend fun poll() { delay(1000) }
```

## kotlin.transactional-suspend

A Spring @Transactional suspend function needs a reactive transaction manager; a blocking one can lose its transaction across suspension points.

- Reports: Spring @Transactional on a Kotlin suspend function.
- Fix: Use a reactive transaction manager with a TransactionalOperator, or move the transactional work into a regular function and call it from the coroutine.
- Suppression: When the application uses a reactive transaction manager (R2DBC); say so in the reason.

Bad:

```kotlin
@Transactional
suspend fun save(book: Book) { ... }
```

Good:

```kotlin
suspend fun save(book: Book) = transactionalOperator.executeAndAwait { ... }
```

## kotlin.unmanaged-coroutine-scope

A class creates a CoroutineScope and never cancels it, so its coroutines keep running after the class is no longer used.

- Reports: A val initialized with CoroutineScope(...) in a class with no function that calls cancel on it.
- Fix: Cancel the scope when the owner shuts down, for example in a @PreDestroy or close() function, or launch the work in a scope with a lifecycle such as coroutineScope { } or a framework-provided scope.
- Suppression: When the class lives as long as the process, such as an application-wide singleton; say so in the reason.

Bad:

```kotlin
class Poller {
    private val scope = CoroutineScope(Dispatchers.Default)
}
```

Good:

```kotlin
class Poller {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    @PreDestroy
    fun close() = scope.cancel()
}
```

## rust.clippy

Clippy reported a lint.

- Reports: Clippy lints (`clippy::...`) from cargo clippy --workspace --all-targets with the project's lint levels. The lint is the diagnostic's subject; a lint the project denies is an error.
- Not reported: Files under target directories.
- Fix: Follow the Clippy suggestion, or allow the lint in code with a reason when it does not apply.
- Suppression: When the lint does not fit the code, allow it on the item with a reason; allow a lint project-wide in [lints] in Cargo.toml instead of suppressing many findings.

Bad:

```rust
if values.len() == 0 {
    return;
}
```

Good:

```rust
if values.is_empty() {
    return;
}
```

## rust.rustc-lint

The Rust compiler reported a lint warning.

- Reports: Compiler lint warnings from cargo clippy, such as unused_variables, dead_code and unused_imports. The lint is the diagnostic's subject.
- Fix: Fix the warning, or allow the lint in code with a reason when it does not apply.
- Suppression: When the code is kept on purpose, for example an item used only on another platform; allow the lint on the item with a reason.

Bad:

```rust
let unused = compute();
```

Good:

```rust
let _unused = compute(); // or remove the binding
```

## security.cors-wildcard

A wildcard CORS origin allows requests from arbitrary origins.

- Reports: A Spring CORS configuration that allows the `*` origin through allowedOrigins("*"), addAllowedOrigin("*") or setAllowedOrigins(List.of("*")).
- Fix: List the trusted origins explicitly.
- Suppression: For a public, unauthenticated, read-only API that sends no credentials; say so in the reason.

Bad:

```java
registry.addMapping("/api/**").allowedOrigins("*");
```

Good:

```java
registry.addMapping("/api/**").allowedOrigins("https://app.example.com");
```

## security.csrf-disabled

Disabling CSRF protection is only safe for stateless APIs that do not use cookie-based sessions.

- Reports: Spring Security csrf configuration that disables CSRF protection.
- Not reported: Methods that also set SessionCreationPolicy.STATELESS.
- Fix: Keep CSRF enabled for browser sessions, or suppress this rule where the API is stateless.
- Suppression: For an API authenticated only by tokens in headers, never cookies; say so in the reason.

Bad:

```java
http.csrf(csrf -> csrf.disable());
```

Good:

```java
http.csrf(Customizer.withDefaults());
```

## security.disabled-tls-verification

This code disables TLS certificate or host name verification.

- Reports: A trust-all X509TrustManager or a HostnameVerifier that always returns true.
- Not reported: Code inside an if or conditional whose condition mentions verif, insecure, trustAll, skip or validat, treated as an opt-in setting. The trust-all implementation itself is still reported.
- Fix: Use the default trust store and host name verification, or trust a specific certificate.
- Suppression: For a trust-all implementation used only behind an explicit opt-in flag for local development; name the flag in the reason.

Bad:

```java
HostnameVerifier any = (host, session) -> true;
```

Good:

```java
// use the default verifier; trust a private CA by adding it to a trust store
```

## security.jackson-default-typing

Jackson reads a class name from the input and can instantiate a class the input chooses.

- Reports: Jackson enableDefaultTyping, activateDefaultTyping with LaissezFaireSubTypeValidator, and @JsonTypeInfo(use = Id.CLASS) or Id.MINIMAL_CLASS, which let the input choose the class to instantiate.
- Not reported: activateDefaultTyping with another validator, and Id.NAME.
- Fix: Use JsonTypeInfo.Id.NAME with @JsonSubTypes to list the allowed subtypes. If you need default typing, pass activateDefaultTyping a BasicPolymorphicTypeValidator that allows only your own packages or base types.
- Suppression: When the JSON never comes from outside the system, such as a local cache file the application writes; say so in the reason.

Bad:

```java
mapper.activateDefaultTyping(LaissezFaireSubTypeValidator.instance);
```

Good:

```java
mapper.activateDefaultTyping(BasicPolymorphicTypeValidator.builder()
    .allowIfSubType("com.example.events.").build());
```

## security.sql-injection

SQL text built by string concatenation or formatting can allow SQL injection.

- Reports: A JDBC, JPA or JdbcTemplate query method whose SQL argument is built by concatenation or formatting and contains SQL text such as select, update or drop in a string literal.
- Not reported: Calls whose string literals contain no SQL keyword, such as signature.update("id=" + name).
- Fix: Use bind parameters such as ? or :name, and pass the values separately.
- Suppression: When the concatenated value is a constant or comes from a fixed allow-list, such as a validated sort column; name the allow-list in the reason.

Bad:

```java
jdbc.query("select * from book where title = '" + title + "'", mapper);
```

Good:

```java
jdbc.query("select * from book where title = ?", mapper, title);
```

## security.weak-crypto

This hash or cipher algorithm is weak or uses an unsafe mode.

- Reports: MessageDigest MD2, MD5 or SHA-1, DigestUtils md5/sha1, and Cipher DES, DESede, RC2, RC4, Blowfish, or AES in ECB or default mode.
- Fix: Use SHA-256 or stronger for hashing and an authenticated mode such as AES/GCM for encryption.
- Suppression: For a non-security checksum an external format requires, such as an ETag or a legacy protocol field; name the format in the reason.

Bad:

```java
MessageDigest.getInstance("MD5");
Cipher.getInstance("AES");
```

Good:

```java
MessageDigest.getInstance("SHA-256");
Cipher.getInstance("AES/GCM/NoPadding");
```

## security.xxe

This XML parser factory accepts DOCTYPE declarations and external entities, which allows XML external entity (XXE) attacks on untrusted input.

- Reports: A DocumentBuilderFactory, SAXParserFactory, XMLInputFactory, TransformerFactory or SchemaFactory created without disabling DOCTYPE declarations or external DTD access later in the same method.
- Not reported: Factories on which the same method sets disallow-doctype-decl to true, XMLConstants.ACCESS_EXTERNAL_DTD to "", or XMLInputFactory.SUPPORT_DTD to false.
- Fix: Disable DOCTYPE declarations on the factory before use, for example factory.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true), or set XMLConstants.ACCESS_EXTERNAL_DTD and ACCESS_EXTERNAL_SCHEMA to "". For XMLInputFactory, set XMLInputFactory.SUPPORT_DTD to false.
- Suppression: When another method configures the factory before use, or the input is a trusted file bundled with the application; name that method or file in the reason.

Bad:

```java
DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
return factory.newDocumentBuilder().parse(input);
```

Good:

```java
DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
factory.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);
return factory.newDocumentBuilder().parse(input);
```

## spring.async-self-invocation

This direct call bypasses the target method's asynchronous advice in Spring proxy mode.

- Reports: A direct Java call, unqualified or through this, to a same-class method annotated with @Async, which runs synchronously because it bypasses the proxy.
- Not reported: Calls through another bean or an injected self reference, and parameterized calls the rule cannot resolve to one declared method.
- Fix: Call the asynchronous operation through another Spring bean.
- Suppression: When running synchronously here is intended; say so in the reason.

Bad:

```java
void handle() { sendMail(); }

@Async
void sendMail() { ... }
```

Good:

```java
void handle() { mailer.sendMail(); } // @Async method on another bean
```

## spring.cache-self-invocation

This direct call bypasses the target method's cache advice in Spring proxy mode.

- Reports: A direct Java call to a same-class method with @Cacheable, @CacheEvict, @CachePut or @Caching, which bypasses the cache.
- Not reported: Calls through another bean or an injected self reference, and parameterized calls the rule cannot resolve to one declared method.
- Fix: Call the cached operation through another Spring bean.
- Suppression: When the caller must read past the cache on purpose; say so in the reason.

Bad:

```java
Book show(long id) { return find(id); }

@Cacheable("books")
Book find(long id) { ... }
```

Good:

```java
Book show(long id) { return bookCache.find(id); } // cached method on another bean
```

## spring.checked-exception-rollback

Review rollback for this checked exception under Spring's default policy.

- Reports: A direct throw of a new IOException or SQLException from a method with a bare or empty Spring @Transactional, in top-level statements or simple top-level if branches (spring-rollback profile). Spring does not roll back for checked exceptions by default.
- Not reported: Methods that set rollbackFor or noRollbackFor.
- Fix: If the failure must roll back the transaction, configure rollbackFor for the exception.
- Suppression: When committing the work done before the exception is intended; say so in the reason.

Bad:

```java
@Transactional
void importBooks() throws IOException {
    throw new IOException("bad file");
}
```

Good:

```java
@Transactional(rollbackFor = IOException.class)
void importBooks() throws IOException { ... }
```

## spring.field-injection

Spring field injection hides a class's dependencies.

- Reports: A Java field annotated with @Autowired, @Inject or @Resource, with or without arguments, static or not.
- Not reported: Setter and constructor injection.
- Fix: Take the dependency as a constructor parameter and store it in a private final field, then remove @Autowired from the field. With Lombok, add @RequiredArgsConstructor to the class and make the field private final. Lombok does not copy @Qualifier from a field to the generated constructor unless lombok.config lists it under lombok.copyableAnnotations. A circular dependency needs a design change instead: extract the shared logic into a third bean.
- Suppression: In a test class that the Spring test framework injects; test sources are skipped by default.

Bad:

```java
@Autowired
private OrderRepository orders;
```

Good:

```java
private final OrderRepository orders;

OrderService(OrderRepository orders) {
    this.orders = orders;
}
```

## spring.http-client-without-timeout

This HTTP client is created with the default settings, which wait for a connection or a response without a time limit.

- Reports: new RestTemplate() without a request factory set right after it, RestClient.create(...) and WebClient.create(...).
- Not reported: Clients built from RestTemplateBuilder, RestClient.builder() or WebClient.builder().
- Fix: Build the client from RestTemplateBuilder, RestClient.builder() or WebClient.builder() with a request factory or connector that sets connect and read timeouts, and share it as a bean.
- Suppression: When the timeouts are configured on the instance elsewhere, such as a customizer bean; name it in the reason.

Bad:

```java
RestTemplate template = new RestTemplate();
```

Good:

```java
RestTemplate template = builder
    .connectTimeout(Duration.ofSeconds(2))
    .readTimeout(Duration.ofSeconds(5))
    .build();
```

## spring.jpa-eager-fetch

This JPA association is fetched eagerly, so every query for the entity loads it, often with one extra query per row.

- Reports: An association or element collection mapped with fetch = FetchType.EAGER.
- Not reported: @ManyToOne and @OneToOne without a fetch attribute, which are eager by JPA default but not reported.
- Fix: Use FetchType.LAZY and load the association where it is needed with a fetch join, an entity graph or a DTO query.
- Suppression: For a small, always-needed reference whose eager load was measured; say so in the reason.

Bad:

```java
@ManyToOne(fetch = FetchType.EAGER)
private Customer customer;
```

Good:

```java
@ManyToOne(fetch = FetchType.LAZY)
private Customer customer; // load it with a fetch join where needed
```

## spring.jpa-entity-request-body

A Spring MVC handler binds request data directly to a JPA entity, so a client can set any of its fields, including ones it must not change (mass assignment).

- Reports: A Spring MVC handler parameter of a JPA entity type bound from the request: @RequestBody, @ModelAttribute, or an unannotated object parameter, including containers such as List<Book>.
- Not reported: @PathVariable and @RequestParam parameters, such as an entity resolved by id.
- Fix: Bind the request to a DTO or record with only the fields the client may set, validate it, and copy those fields onto the entity in the service.
- Suppression: When every entity field is safe for the client to set, such as an internal admin tool; say so in the reason.

Bad:

```java
@PostMapping("/orders")
Order place(@RequestBody Order order) { ... }
```

Good:

```java
record PlaceOrder(String productId, int quantity) {}

@PostMapping("/orders")
OrderView place(@RequestBody @Valid PlaceOrder request) { ... }
```

## spring.jpa-entity-response

This Spring MVC endpoint declares a JPA entity as a response type.

- Reports: A JPA entity declared as the response type of a Spring MVC endpoint: @RestController, @Controller with @ResponseBody, or an explicit ResponseEntity, following arrays and containers such as ResponseEntity<List<Book>> (jpa-web profile).
- Fix: Map the entity to a response DTO with the intended API fields.
- Suppression: When the entity is a deliberate read model with only public fields; say so in the reason.

Bad:

```java
@GetMapping("/books/{id}")
Book show(@PathVariable long id) { ... }
```

Good:

```java
@GetMapping("/books/{id}")
BookView show(@PathVariable long id) { return BookView.from(books.find(id)); }
```

## spring.jpa-lombok-entity

Lombok generates equals, hashCode or toString for this JPA entity from all of its fields, which loads lazy associations, can recurse through bidirectional relations and changes the hash code when the entity is saved.

- Reports: A JPA @Entity class with Lombok @Data, or @EqualsAndHashCode or @ToString without arguments.
- Not reported: Those annotations with arguments such as onlyExplicitlyIncluded = true, and Lombok on classes that are not entities.
- Fix: Remove @Data, @EqualsAndHashCode and @ToString from the entity. Use @Getter and @Setter, base equality on the identifier with @EqualsAndHashCode(onlyExplicitlyIncluded = true), and list the fields toString prints.
- Suppression: For an entity with no associations and an assigned natural identifier; say so in the reason.

Bad:

```java
@Data
@Entity
class Order { ... }
```

Good:

```java
@Entity
@Getter
@Setter
@EqualsAndHashCode(onlyExplicitlyIncluded = true)
class Order {
    @Id
    @EqualsAndHashCode.Include
    private Long id;
}
```

## spring.open-in-view-enabled

This Spring Boot application uses Spring Data JPA and leaves spring.jpa.open-in-view at its default of true, so every web request holds a database connection until the response is written and lazy loading can run queries from the view.

- Reports: A @SpringBootApplication class in a module whose build file, or a parent build file, names spring-boot-starter-data-jpa, and whose src/main/resources/application.properties, application.yml or application.yaml does not set spring.jpa.open-in-view to false outside a profile-specific document. With --changed it runs only when the application class changes.
- Fix: Set spring.jpa.open-in-view=false in application.properties or application.yml, and load what each response needs in the service with fetch joins, entity graphs or DTO queries.
- Suppression: When the property is set elsewhere, such as environment variables or a config server; say where in the reason.

Bad:

```yaml
# application.yml
spring:
  jpa:
    show-sql: false
```

Good:

```yaml
# application.yml
spring:
  jpa:
    open-in-view: false
```

## spring.private-transactional

Spring proxy mode cannot apply transaction advice to a private method.

- Reports: A private Java method with Spring's @Transactional, which a proxy cannot advise.
- Fix: Place the transaction boundary on a method invoked through a Spring proxy.
- Suppression: Do not suppress; the annotation has no effect.

Bad:

```java
@Transactional
private void save() {}
```

Good:

```java
@Transactional
public void save() {} // called through the bean
```

## spring.redundant-autowired-constructor

The class's only constructor is annotated with @Autowired, which Spring does not need for a single constructor.

- Reports: @Autowired on the only constructor of a class.
- Not reported: @Autowired on one of several constructors, where it selects the constructor Spring uses.
- Fix: Remove @Autowired from the constructor.
- Suppression: Not needed; remove the annotation.

Bad:

```java
@Autowired
CheckoutController(OrderService orders) { ... }
```

Good:

```java
CheckoutController(OrderService orders) { ... }
```

## spring.request-mapping-without-method

This @RequestMapping handler names no HTTP method, so it answers every method, including state-changing requests sent as GET.

- Reports: A handler method annotated with @RequestMapping without method, which answers every HTTP method.
- Not reported: Class-level @RequestMapping, which only shares a path prefix.
- Fix: Use @GetMapping, @PostMapping, @PutMapping, @PatchMapping or @DeleteMapping, or set method = RequestMethod.X on the @RequestMapping.
- Suppression: When the endpoint must accept every method, such as a webhook receiver; say so in the reason.

Bad:

```java
@RequestMapping("/cancel")
String cancel() { ... }
```

Good:

```java
@PostMapping("/cancel")
String cancel() { ... }
```

## spring.retry-self-invocation

This direct call bypasses the target method's retry advice in Spring proxy mode.

- Reports: A direct Java call to a same-class method with Spring Retry's @Retryable, which runs once without retries.
- Not reported: Calls through another bean or an injected self reference, and parameterized calls the rule cannot resolve to one declared method.
- Fix: Call the retryable operation through another Spring bean.
- Suppression: When a single attempt is intended here; say so in the reason.

Bad:

```java
void sync() { push(); }

@Retryable
void push() { ... }
```

Good:

```java
void sync() { pusher.push(); } // @Retryable method on another bean
```

## spring.shared-simple-date-format

A Spring singleton holds a non-thread-safe SimpleDateFormat.

- Reports: A field of type SimpleDateFormat, or initialized with one, in a singleton Spring component.
- Not reported: Method-local formatters and request-, session-, application- and prototype-scoped beans.
- Fix: Use an immutable DateTimeFormatter or create the formatter for each operation.
- Suppression: When every use is synchronized on the formatter; say so in the reason.

Bad:

```java
@Service
class Report {
    private final SimpleDateFormat format = new SimpleDateFormat("yyyy-MM-dd");
}
```

Good:

```java
@Service
class Report {
    private static final DateTimeFormatter FORMAT = DateTimeFormatter.ofPattern("yyyy-MM-dd");
}
```

## spring.singleton-request-state

This singleton controller stores a request-handler parameter in an instance field.

- Reports: A Java Spring MVC singleton controller handler that stores one of its parameters directly in a this field, sharing request data between concurrent requests.
- Not reported: Request- and prototype-scoped controllers.
- Fix: Keep request data in local variables or a request-scoped object.
- Suppression: When the field is thread-safe and meant to hold the latest value, such as a metrics gauge; say so in the reason.

Bad:

```java
@GetMapping("/books")
String handler(@RequestParam String user) {
    this.lastUser = user;
    return render(user);
}
```

Good:

```java
@GetMapping("/books")
String handler(@RequestParam String user) {
    return render(user);
}
```

## spring.static-member-injection

Spring does not inject a static field or static method, so the value stays unset.

- Reports: Spring's @Value on a static field or static method, or @Autowired on a static method.
- Not reported: @Autowired fields, static or not, which spring.field-injection reports.
- Fix: Make the field or setter an instance member of the bean, or bind the value with @ConfigurationProperties and pass the bean where it is needed.
- Suppression: Do not suppress; Spring never sets the value.

Bad:

```java
@Value("${app.name}")
private static String name;
```

Good:

```java
private final String name;

AppSettings(@Value("${app.name}") String name) {
    this.name = name;
}
```

## spring.transactional-in-controller

A Spring MVC controller declares a transaction, which holds the database connection while the response is rendered and mixes web handling with the unit of work.

- Reports: Spring or JTA @Transactional on a @Controller or @RestController class or on one of its methods.
- Fix: Move @Transactional to the service method the controller calls, and keep the controller to request mapping and response building.
- Suppression: For a thin controller in a prototype or admin tool without a service layer; say so in the reason.

Bad:

```java
@RestController
class CheckoutController {
    @Transactional
    @PostMapping("/orders")
    String place(...) { ... }
}
```

Good:

```java
@Service
class CheckoutService {
    @Transactional
    void place(...) { ... }
}
```

## spring.transactional-self-invocation

This direct call bypasses the target method's transaction advice in Spring proxy mode.

- Reports: A direct Java call, unqualified or through this, to a same-class method with Spring's @Transactional, which runs without the annotation's transaction settings.
- Not reported: Calls through another bean or an injected self reference, and parameterized calls the rule cannot resolve to one declared method.
- Fix: Move the transaction boundary to a proxied entry point or invoke the operation on another Spring bean.
- Suppression: When the caller already runs in the same transaction and the callee's settings add nothing; say so in the reason.

Bad:

```java
void importAll() { importOne(); }

@Transactional(propagation = REQUIRES_NEW)
void importOne() { ... }
```

Good:

```java
void importAll() { importer.importOne(); } // @Transactional method on another bean
```

## thymeleaf.restricted-bean-access

Thymeleaf 3.1.4 and later restrict Spring bean access in this expression.

- Reports: A Spring bean reference (`${@bean...}`) in a direct expression of th:href, th:src, th:utext, th:on*, th:insert or th:replace and their data-th-* forms, including simple dynamic URL bases and fragment selectors. Thymeleaf 3.1.4 and later can reject these at render time.
- Fix: Compute the value in the controller and expose it as a model attribute.
- Suppression: Do not suppress; compute the value in the controller and expose it as a model attribute.

Bad:

```html
<a th:href="${@bookLinks.list()}">Books</a>
```

Good:

```html
<a th:href="${booksUrl}">Books</a> <!-- the controller adds booksUrl to the model -->
```

## thymeleaf.unescaped-output

Thymeleaf renders this expression without HTML escaping.

- Reports: A dynamic expression rendered without HTML escaping: th:utext, data-th-utext, or inline [(...)] in template text (thymeleaf-output-review profile).
- Not reported: Expressions passed through a sanitizer listed in thymeleaf_sanitizers, and literal values.
- Fix: Use escaped output or verify that the HTML has been sanitized before rendering.
- Suppression: When the value is trusted HTML or already sanitized; name the source or sanitizer in the reason, or add the sanitizer to thymeleaf_sanitizers.
- Configuration: thymeleaf_sanitizers

Bad:

```html
<div th:utext="${comment.body}"></div>
```

Good:

```html
<div th:text="${comment.body}"></div>
```

## typescript.no-floating-promises

A Promise is neither awaited nor handled, so its rejection goes unnoticed and the code after it does not wait for it.

- Reports: An expression statement whose value TypeScript types as a Promise, or an array of Promises, that is not awaited, returned, or given a rejection handler (typescript-eslint no-floating-promises, with type information; typescript-review profile).
- Not reported: A call prefixed with void, a Promise with .catch() or a two-argument .then(), and TypeScript files that no tsconfig.json project includes; the report notes how many files that skips. Values whose types cannot be resolved, such as imports from packages that are not installed.
- Fix: Await the Promise, return it, or handle its rejection with .catch() or a two-argument .then(). Mark an intentional fire-and-forget call with the void operator.
- Suppression: Prefer void for an intentional fire-and-forget call. Suppress only when a framework handles the Promise in a way the types do not show; say so in the reason.

Bad:

```java
async function save(order: Order): Promise<void> { /* ... */ }

save(order);
```

Good:

```java
await save(order);
// or, intentionally in the background:
void save(order).catch(report);
```
