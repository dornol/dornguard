package io.dornguard.runner;

import com.google.gson.Gson;
import com.tngtech.archunit.core.domain.Dependency;
import com.tngtech.archunit.core.domain.JavaClass;
import com.tngtech.archunit.core.importer.ClassFileImporter;
import java.io.IOException;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;

public final class Main {
    private static final int SCHEMA_VERSION = 3;

    private Main() {}

    public static void main(String[] args) throws IOException {
        if (args.length == 1 && args[0].equals("--version")) {
            System.out.println(SCHEMA_VERSION);
            return;
        }
        if (args.length != 4 || !args[0].equals("--config") || !args[2].equals("--output")) {
            throw new IllegalArgumentException("Usage: archunit-runner --config request.json --output result.json");
        }

        Gson gson = new Gson();
        Request request;
        try (var reader = Files.newBufferedReader(Path.of(args[1]))) {
            request = gson.fromJson(reader, Request.class);
        }
        if (request == null || request.schemaVersion() != SCHEMA_VERSION) {
            throw new IllegalArgumentException("Unsupported ArchUnit runner request schema");
        }

        List<Finding> findings = new ArrayList<>();
        List<String> analyzed = new ArrayList<>();
        List<Failure> failures = new ArrayList<>();
        for (Module module : request.modules()) {
            try {
                check(module, findings);
                analyzed.add(module.path());
            } catch (IOException | RuntimeException exception) {
                failures.add(new Failure(module.path(), exception.getClass().getSimpleName()));
            }
        }

        findings.sort(Comparator.comparing(Finding::module)
                .thenComparing(Finding::ruleId)
                .thenComparing(Finding::path, Comparator.nullsFirst(Comparator.naturalOrder()))
                .thenComparing(Finding::line, Comparator.nullsFirst(Comparator.naturalOrder()))
                .thenComparing(Finding::origin)
                .thenComparing(Finding::target));
        try (var writer = Files.newBufferedWriter(Path.of(args[3]))) {
            gson.toJson(new Report(SCHEMA_VERSION, analyzed, findings, failures), writer);
        }
    }

    private static void check(Module module, List<Finding> findings) throws IOException {
        List<Path> classes = module.classDirs().stream().map(Path::of).filter(Files::isDirectory).toList();
        var sourceIndex = new SourceIndex(module.sourceFiles(), classes);
        var resolverClasspath = new ArrayList<URL>();
        for (Path path : classes) {
            resolverClasspath.add(path.toUri().toURL());
        }
        for (String path : module.classpath()) {
            resolverClasspath.add(Path.of(path).toUri().toURL());
        }

        Thread current = Thread.currentThread();
        ClassLoader previous = current.getContextClassLoader();
        try (var classpath = new URLClassLoader(resolverClasspath.toArray(URL[]::new), ClassLoader.getPlatformClassLoader())) {
            current.setContextClassLoader(classpath);
            var imported = new ClassFileImporter().importPaths(classes);
            if (imported.isEmpty()) {
                throw new IllegalStateException("No classes were imported");
            }
            for (JavaClass origin : imported) {
                SourceIndex.Origin source = null;
                for (Rule rule : module.rules()) {
                    if (!rule.origins().isEmpty() && !matches(origin.getPackageName(), rule.origins())) {
                        continue;
                    }
                    for (Dependency dependency : origin.getDirectDependenciesFromSelf()) {
                        JavaClass target = dependency.getTargetClass();
                        if (matches(target.getPackageName(), rule.forbidden())) {
                            if (source == null) {
                                source = sourceIndex.locate(origin.getName(), origin.getPackageName());
                            }
                            if (source.kotlin() && !module.kotlin()) {
                                continue;
                            }
                            var location = dependency.getSourceCodeLocation();
                            findings.add(new Finding(
                                    module.path(), rule.id(), source.path(),
                                    source.line(location.getLineNumber()), origin.getName(), target.getName(),
                                    dependency.getDescription().replaceFirst(" in \\(.*:[0-9]+\\)$", "")));
                        }
                    }
                }
            }
        } finally {
            current.setContextClassLoader(previous);
        }
    }

    private static boolean matches(String packageName, List<String> selectors) {
        List<String> components = Arrays.asList(packageName.split("\\."));
        for (String selector : selectors) {
            if (selector.indexOf('.') < 0) {
                if (components.contains(selector)) {
                    return true;
                }
            } else if (packageName.equals(selector) || packageName.startsWith(selector + ".")) {
                return true;
            }
        }
        return false;
    }

    private record Request(int schemaVersion, List<Module> modules) {}
    private record Module(String path, List<String> sourceFiles, boolean kotlin,
                          List<String> classDirs, List<String> classpath, List<Rule> rules) {}
    private record Rule(String id, List<String> origins, List<String> forbidden) {}
    private record Finding(String module, String ruleId, String path, Integer line, String origin, String target, String identity) {}
    private record Failure(String module, String reason) {}
    private record Report(int schemaVersion, List<String> analyzed, List<Finding> findings, List<Failure> failures) {}
}
