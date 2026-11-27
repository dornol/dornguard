# Rule catalog

<!-- Generated from the built-in rules. Regenerate with
DORNGUARD_BLESS=1 cargo test -p dornguard-config rule_catalog -->

Every built-in rule with its engine, default severity, confidence and
the profiles that enable it. Run `dornguard rules explain RULE` for the
fix guidance. See [policy bundles](profiles.md) for what each profile
covers and [rule confidence](profiles.md#rule-confidence) for how to treat findings.

| Rule | Engine | Severity | Confidence | Profiles | Description |
| --- | --- | --- | --- | --- | --- |
| `architecture.adapter-isolation` | archunit | error | high | `hexagonal` | An inbound adapter must not depend on an outbound adapter. |
| `architecture.application-independence` | archunit | error | high | `hexagonal` | Application code must not depend on adapters or infrastructure. |
| `architecture.controller-repository` | archunit | error | high | `layered` | A controller must not depend directly on a repository. |
| `architecture.domain-independence` | archunit | error | high | `hexagonal` | Domain code must remain independent of application and infrastructure. |
| `architecture.framework-independence` | archunit | error | high | `hexagonal` | Domain and application code must not reference the Spring framework. |
| `architecture.library-framework-independence` | archunit | error | high | `java-library` | A Java library must not reference the Spring framework. |
| `architecture.module-dependency` | gradle | error | high | `module_dependencies` | A forbidden Gradle project dependency is declared. |
| `architecture.package-cycle` | archunit | error | high | `package-cycles` | Packages must not depend on each other in a cycle. |
| `architecture.repository-independence` | archunit | error | high | `layered` | Repositories must remain independent of service and controller layers. |
| `architecture.service-independence` | archunit | error | high | `layered` | A service must not depend on the controller layer. |
| `flyway.changed-versioned-migration` | builtin | warning | high | `flyway` | An existing Flyway versioned migration changed or was removed. |
| `format.blank-lines` | builtin | error | high | `format` | This source has too many consecutive blank lines. |
| `format.control-spacing` | builtin | error | high | `format` | Use one space before a control statement's opening parenthesis. |
| `format.final-newline` | builtin | error | high | `format` | End nonempty files with a newline. |
| `format.indentation` | builtin | error | high | `format` | Use spaces and the configured indentation for this language. |
| `format.line-ending` | builtin | error | high | `format` | This file uses a different line ending than the configured format. |
| `format.trailing-whitespace` | builtin | error | high | `format` | Remove trailing spaces and tabs. |
| `go.vet` | go_vet | error | high | `go` | go vet reported a suspicious construct. |
| `html.inline-event-handler` | semgrep | warning | high | `html` | This HTML or Thymeleaf attribute contains an inline event handler. |
| `html.inline-script` | semgrep | warning | high | `html` | This HTML script contains inline JavaScript. |
| `java.assert-throws-nested-call` | semgrep | warning | medium | `java-review` | A call that prepares an argument can throw before JUnit reaches the operation under test. |
| `java.bigdecimal-from-floating-point-literal` | pmd | warning | high | `java` | Constructing BigDecimal from a floating-point literal can introduce a rounding error. |
| `java.broken-null-check` | pmd | warning | medium | `java` | This null check can still evaluate an operation on a null reference. |
| `java.cognitive-complexity` | pmd | warning | medium | `java-design` | This method's cognitive complexity is 15 or more, which makes it hard to read and test. |
| `java.date-pattern-typo` | semgrep | error | high | `java-source` | A date pattern uses the week-based year (YYYY) or the day of the year (DD) by mistake. |
| `java.double-checked-locking` | pmd | warning | high | `java` | Double-checked locking on a non-volatile field can publish a partly constructed object. |
| `java.empty-catch` | pmd | error | high | `java` | Handle exceptions explicitly. |
| `java.equals-hashcode-contract` | pmd | warning | high | `java` | Overriding equals or hashCode alone can break hash-based collections. |
| `java.excessive-parameters` | pmd | warning | medium | `java-design` | This method or constructor declares eight or more parameters. |
| `java.fully-qualified-type` | semgrep | warning | medium | `java-source` | This reference looks like a fully qualified type name. |
| `java.god-class` | pmd | warning | medium | `java-design` | This class is large, complex and uses many other classes' data, which suggests it has too many responsibilities. |
| `java.ignored-interrupt` | semgrep | warning | medium | `java-source` | An InterruptedException is logged and consumed. |
| `java.ignored-return-value` | pmd | warning | medium | `java-review` | This call discards the result of a method whose return value should be used. |
| `java.jspecify-null-marked` | builtin | error | high | `jspecify` | Java types must be in an org.jspecify.annotations.NullMarked scope. |
| `java.lock-without-finally` | semgrep | warning | medium | `java-source` | A lock is acquired without a try/finally that releases it, so an exception keeps it held. |
| `java.log-format-arguments` | pmd | warning | high | `java` | The number of {} placeholders in a log message does not match its arguments. |
| `java.loose-coupling` | pmd | warning | high | `java-design` | A field, parameter or return type names a collection implementation such as ArrayList or HashMap instead of its interface. |
| `java.optional-misuse` | semgrep | warning | medium | `java-source` | Optional is used as a field or parameter type, or a method that returns Optional returns null. |
| `java.preserve-stack-trace` | pmd | warning | medium | `java` | This exception drops the original cause. |
| `java.print-stack-trace` | semgrep | error | high | `java-source` | Avoid printing stack traces to standard error. |
| `java.resource-not-closed` | pmd | warning | medium | `java` | The code may leave a Java resource open. |
| `java.return-from-finally` | pmd | warning | high | `java` | Returning from finally can suppress an exception or replace the original return value. |
| `java.return-null-collection` | pmd | warning | high | `java` | A method that returns a collection or array returns null. |
| `java.stdout-usage` | semgrep | error | high | `java-source` | Avoid printing directly to standard output. |
| `java.string-reference-equality` | pmd | error | high | `java` | Strings are compared with == or != instead of equals. |
| `java.throw-from-finally` | pmd | warning | high | `java` | Throwing from finally replaces the exception or return value of the try block. |
| `java.unnecessary-import` | pmd | warning | high | `java` | This Java import is unnecessary. |
| `java.unused-local-variable` | pmd | warning | high | `java` | This local variable is assigned but never used. |
| `java.unused-private-field` | pmd | warning | high | `java` | This private field is never read. |
| `java.unused-private-method` | pmd | warning | high | `java` | This private method is never called. |
| `java.wildcard-import` | semgrep | warning | high | `java-source` | A wildcard import hides which types the file uses and can make a new type in the package collide with another import. |
| `javascript.no-async-promise-executor` | eslint | warning | high | `javascript` | An async Promise executor can leave the outer Promise pending if it throws. |
| `javascript.no-constant-binary-expression` | eslint | warning | high | `javascript` | A comparison or logical expression has a result that does not depend on its operands. |
| `javascript.no-eval` | eslint | warning | high | `javascript` | Avoid evaluating strings as JavaScript with eval. |
| `javascript.no-implied-eval` | eslint | warning | high | `javascript` | Avoid sending JavaScript strings to APIs that evaluate them. |
| `javascript.no-new-func` | eslint | warning | high | `javascript` | Avoid creating functions from strings. |
| `javascript.no-script-url` | eslint | warning | high | `javascript` | A javascript: URL runs code like eval and is blocked by a strict Content Security Policy. |
| `javascript.no-self-compare` | eslint | warning | high | `javascript` | A value is compared with itself, which is always true or false except for NaN. |
| `javascript.no-unsafe-finally` | eslint | warning | high | `javascript` | A return, throw, break or continue in finally replaces the outcome of the try block. |
| `javascript.no-unsafe-negation` | eslint | warning | high | `javascript` | The ! operator applies to the left operand of in or instanceof instead of the whole test. |
| `javascript.no-unsafe-optional-chaining` | eslint | warning | high | `javascript` | An optional chain that can be undefined is used where undefined throws a TypeError. |
| `javascript.use-isnan` | eslint | warning | high | `javascript` | A comparison with NaN is always false. |
| `javascript.valid-typeof` | eslint | warning | high | `javascript` | typeof is compared with a string it never returns. |
| `jsp.scripting-element` | semgrep | warning | high | `jsp` | This JSP contains embedded scripting code. |
| `jsp.unescaped-output` | semgrep | warning | medium | `jsp` | This JSTL output explicitly disables XML escaping. |
| `kotlin.date-pattern-typo` | semgrep | error | high | `kotlin` | A date pattern uses the week-based year (YYYY) or the day of the year (DD) by mistake. |
| `kotlin.field-injection` | semgrep | error | high | `kotlin` | Spring field injection hides a class's dependencies. |
| `kotlin.fully-qualified-type` | semgrep | warning | medium | `kotlin` | This reference looks like a fully qualified type name. |
| `kotlin.global-scope` | semgrep | warning | high | `kotlin` | GlobalScope launches work outside the caller's lifecycle. |
| `kotlin.jpa-data-class-entity` | semgrep | warning | high | `kotlin` | A Kotlin data class is used as a JPA entity. |
| `kotlin.missing-jpa-plugin` | gradle | error | high | `kotlin` | A Gradle module declares Kotlin JPA entities without the Kotlin JPA compiler plugin, so the entities have no no-arg constructor for JPA to instantiate them. |
| `kotlin.missing-spring-plugin` | gradle | error | high | `kotlin` | A Gradle module declares final Kotlin classes that Spring must proxy, such as @Configuration or @Transactional classes, without the Kotlin Spring compiler plugin. |
| `kotlin.not-null-assertion` | semgrep | warning | high | `kotlin-review` | The not-null assertion operator (!!) throws a NullPointerException with no context when the value is null. |
| `kotlin.print-stack-trace` | semgrep | error | high | `kotlin` | Avoid printing stack traces to standard error. |
| `kotlin.private-transactional` | semgrep | warning | high | `kotlin` | Spring proxy mode cannot apply transaction advice to a private function. |
| `kotlin.run-blocking-in-suspend` | semgrep | warning | high | `kotlin` | runBlocking blocks the thread inside a suspend function. |
| `kotlin.run-catching-cancellation` | semgrep | warning | medium | `kotlin` | runCatching can store cancellation from this suspending coroutine operation as a failure. |
| `kotlin.sql-injection` | semgrep | error | medium | `kotlin` | SQL text built by string interpolation, concatenation, or formatting can allow SQL injection. |
| `kotlin.stdout-usage` | semgrep | error | high | `kotlin` | Avoid printing directly to standard output. |
| `kotlin.swallowed-cancellation` | semgrep | warning | medium | `kotlin` | This catch consumes a coroutine cancellation exception. |
| `kotlin.thread-sleep-in-suspend` | semgrep | warning | high | `kotlin` | Thread.sleep blocks the thread inside a suspend function. |
| `kotlin.transactional-suspend` | semgrep | warning | high | `kotlin` | A Spring @Transactional suspend function needs a reactive transaction manager; a blocking one can lose its transaction across suspension points. |
| `kotlin.unmanaged-coroutine-scope` | semgrep | warning | medium | `kotlin` | A class creates a CoroutineScope and never cancels it, so its coroutines keep running after the class is no longer used. |
| `rust.clippy` | clippy | warning | high | `rust` | Clippy reported a lint. |
| `rust.rustc-lint` | clippy | warning | high | `rust` | The Rust compiler reported a lint warning. |
| `security.cors-wildcard` | semgrep | error | medium | `security` | A wildcard CORS origin allows requests from arbitrary origins. |
| `security.csrf-disabled` | semgrep | warning | medium | `security` | Disabling CSRF protection is only safe for stateless APIs that do not use cookie-based sessions. |
| `security.disabled-tls-verification` | semgrep | error | medium | `security` | This code disables TLS certificate or host name verification. |
| `security.jackson-default-typing` | semgrep | warning | medium | `security` | Jackson reads a class name from the input and can instantiate a class the input chooses. |
| `security.sql-injection` | semgrep | error | medium | `security` | SQL text built by string concatenation or formatting can allow SQL injection. |
| `security.weak-crypto` | semgrep | warning | medium | `security` | This hash or cipher algorithm is weak or uses an unsafe mode. |
| `security.xxe` | semgrep | warning | medium | `security` | This XML parser factory accepts DOCTYPE declarations and external entities, which allows XML external entity (XXE) attacks on untrusted input. |
| `spring.async-self-invocation` | builtin | warning | high | `spring-boot` | This direct call bypasses the target method's asynchronous advice in Spring proxy mode. |
| `spring.cache-self-invocation` | builtin | warning | high | `spring-boot` | This direct call bypasses the target method's cache advice in Spring proxy mode. |
| `spring.checked-exception-rollback` | semgrep | warning | medium | `spring-rollback` | Review rollback for this checked exception under Spring's default policy. |
| `spring.field-injection` | semgrep | error | high | `spring-boot` | Spring field injection hides a class's dependencies. |
| `spring.http-client-without-timeout` | semgrep | warning | medium | `spring-boot` | This HTTP client is created with the default settings, which wait for a connection or a response without a time limit. |
| `spring.jpa-eager-fetch` | semgrep | warning | high | `spring-boot` | This JPA association is fetched eagerly, so every query for the entity loads it, often with one extra query per row. |
| `spring.jpa-entity-request-body` | builtin | warning | high | `spring-boot` | A Spring MVC handler binds request data directly to a JPA entity, so a client can set any of its fields, including ones it must not change (mass assignment). |
| `spring.jpa-entity-response` | builtin | warning | medium | `jpa-web` | This Spring MVC endpoint declares a JPA entity as a response type. |
| `spring.jpa-lombok-entity` | semgrep | warning | high | `spring-boot` | Lombok generates equals, hashCode or toString for this JPA entity from all of its fields, which loads lazy associations, can recurse through bidirectional relations and changes the hash code when the entity is saved. |
| `spring.open-in-view-enabled` | builtin | warning | medium | `spring-boot` | This Spring Boot application uses Spring Data JPA and leaves spring.jpa.open-in-view at its default of true, so every web request holds a database connection until the response is written and lazy loading can run queries from the view. |
| `spring.private-transactional` | semgrep | warning | high | `spring-boot` | Spring proxy mode cannot apply transaction advice to a private method. |
| `spring.redundant-autowired-constructor` | semgrep | warning | high | `spring-boot` | The class's only constructor is annotated with @Autowired, which Spring does not need for a single constructor. |
| `spring.request-mapping-without-method` | semgrep | warning | high | `spring-boot` | This @RequestMapping handler names no HTTP method, so it answers every method, including state-changing requests sent as GET. |
| `spring.retry-self-invocation` | builtin | warning | high | `spring-boot` | This direct call bypasses the target method's retry advice in Spring proxy mode. |
| `spring.shared-simple-date-format` | semgrep | warning | medium | `spring-boot` | A Spring singleton holds a non-thread-safe SimpleDateFormat. |
| `spring.singleton-request-state` | semgrep | warning | medium | `spring-boot` | This singleton controller stores a request-handler parameter in an instance field. |
| `spring.static-member-injection` | semgrep | error | high | `spring-boot` | Spring does not inject a static field or static method, so the value stays unset. |
| `spring.transactional-in-controller` | semgrep | warning | high | `spring-boot` | A Spring MVC controller declares a transaction, which holds the database connection while the response is rendered and mixes web handling with the unit of work. |
| `spring.transactional-self-invocation` | builtin | warning | high | `spring-boot` | This direct call bypasses the target method's transaction advice in Spring proxy mode. |
| `thymeleaf.restricted-bean-access` | semgrep | error | high | `thymeleaf` | Thymeleaf 3.1.4 and later restrict Spring bean access in this expression. |
| `thymeleaf.unescaped-output` | semgrep | warning | medium | `thymeleaf-output-review` | Thymeleaf renders this expression without HTML escaping. |
