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
import java.util.ArrayDeque;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;

public final class Main {
    private static final int SCHEMA_VERSION = 5;

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
                var moduleFindings = new ArrayList<Finding>();
                Failure failure = check(module, moduleFindings);
                if (failure == null) {
                    findings.addAll(moduleFindings);
                    analyzed.add(module.path());
                } else {
                    failures.add(failure);
                }
            } catch (IOException | RuntimeException exception) {
                failures.add(new Failure(module.path(), null, exception.getClass().getSimpleName()));
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

    private static Failure check(Module module, List<Finding> findings) throws IOException {
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
                return new Failure(module.path(), null, "No class files were imported. Check the compiler output directory.");
            }
            var coveredSources = new TreeSet<String>();
            var ownClasses = new HashSet<String>();
            for (JavaClass candidate : imported) {
                ownClasses.add(candidate.getName());
            }
            var cycles = new HashMap<Rule, Map<String, Integer>>();
            for (Rule rule : module.rules()) {
                if (rule.cycles()) {
                    cycles.put(rule, cyclicPackages(imported, ownClasses, rule.origins()));
                }
            }
            for (JavaClass origin : imported) {
                SourceIndex.Origin source = sourceIndex.locate(origin.getName(), origin.getPackageName());
                if (source.path() != null) {
                    coveredSources.add(source.path());
                    if (module.checkClassTimestamps()
                            && source.classTime().compareTo(Files.getLastModifiedTime(Path.of(source.path()))) < 0) {
                        return new Failure(module.path(), source.path(),
                                "A class file is older than its source. Recompile the module and rerun the check.");
                    }
                }
                for (Rule rule : module.rules()) {
                    if (!rule.origins().isEmpty() && !matches(origin.getPackageName(), rule.origins())) {
                        continue;
                    }
                    var cyclic = cycles.get(rule);
                    for (Dependency dependency : origin.getDirectDependenciesFromSelf()) {
                        JavaClass target = dependency.getTargetClass();
                        boolean reported = cyclic == null
                                ? matches(target.getPackageName(), rule.forbidden())
                                : inSameCycle(cyclic, origin.getPackageName(), target.getPackageName())
                                        && ownClasses.contains(target.getName());
                        if (reported) {
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
            for (String file : module.sourceFiles()) {
                if (!coveredSources.contains(file) && sourceIndex.expectsJavaClass(Path.of(file))) {
                    return new Failure(module.path(), file,
                            "No class file could be matched to this Java source. Check the compiler output directory.");
                }
            }
            return null;
        } finally {
            current.setContextClassLoader(previous);
        }
    }

    private static boolean inSameCycle(Map<String, Integer> components, String from, String to) {
        Integer fromId = components.get(from);
        return fromId != null && !from.equals(to) && fromId.equals(components.get(to));
    }

    /**
     * Maps each package that belongs to a dependency cycle to its strongly connected component.
     * Only dependencies between classes of the module are considered, and packages are
     * restricted to the rule's selectors when it has any.
     */
    private static Map<String, Integer> cyclicPackages(
            Iterable<JavaClass> classes, Set<String> ownClasses, List<String> selectors) {
        var graph = new TreeMap<String, Set<String>>();
        for (JavaClass origin : classes) {
            String from = origin.getPackageName();
            if (!selectors.isEmpty() && !matches(from, selectors)) {
                continue;
            }
            for (Dependency dependency : origin.getDirectDependenciesFromSelf()) {
                JavaClass target = dependency.getTargetClass();
                String to = target.getPackageName();
                if (!ownClasses.contains(target.getName()) || from.equals(to)
                        || (!selectors.isEmpty() && !matches(to, selectors))) {
                    continue;
                }
                graph.computeIfAbsent(from, key -> new TreeSet<>()).add(to);
                graph.computeIfAbsent(to, key -> new TreeSet<>());
            }
        }
        return stronglyConnected(graph);
    }

    /** Tarjan's algorithm without recursion; returns only components with more than one package. */
    private static Map<String, Integer> stronglyConnected(Map<String, Set<String>> graph) {
        var index = new HashMap<String, Integer>();
        var low = new HashMap<String, Integer>();
        var onStack = new HashSet<String>();
        var stack = new ArrayDeque<String>();
        var result = new TreeMap<String, Integer>();
        int[] counter = {0, 0};
        for (String root : graph.keySet()) {
            if (index.containsKey(root)) {
                continue;
            }
            var work = new ArrayDeque<Map.Entry<String, java.util.Iterator<String>>>();
            index.put(root, counter[0]);
            low.put(root, counter[0]++);
            stack.push(root);
            onStack.add(root);
            work.push(Map.entry(root, graph.get(root).iterator()));
            while (!work.isEmpty()) {
                var frame = work.peek();
                String node = frame.getKey();
                if (frame.getValue().hasNext()) {
                    String next = frame.getValue().next();
                    if (!index.containsKey(next)) {
                        index.put(next, counter[0]);
                        low.put(next, counter[0]++);
                        stack.push(next);
                        onStack.add(next);
                        work.push(Map.entry(next, graph.get(next).iterator()));
                    } else if (onStack.contains(next)) {
                        low.put(node, Math.min(low.get(node), index.get(next)));
                    }
                    continue;
                }
                work.pop();
                if (!work.isEmpty()) {
                    String parent = work.peek().getKey();
                    low.put(parent, Math.min(low.get(parent), low.get(node)));
                }
                if (low.get(node).equals(index.get(node))) {
                    var component = new ArrayList<String>();
                    String member;
                    do {
                        member = stack.pop();
                        onStack.remove(member);
                        component.add(member);
                    } while (!member.equals(node));
                    if (component.size() > 1) {
                        int id = counter[1]++;
                        component.forEach(name -> result.put(name, id));
                    }
                }
            }
        }
        return result;
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
                          boolean checkClassTimestamps, List<String> classDirs,
                          List<String> classpath, List<Rule> rules) {}
    private record Rule(String id, List<String> origins, List<String> forbidden, boolean cycles) {}
    private record Finding(String module, String ruleId, String path, Integer line, String origin, String target, String identity) {}
    private record Failure(String module, String path, String reason) {}
    private record Report(int schemaVersion, List<String> analyzed, List<Finding> findings, List<Failure> failures) {}
}
