package io.dornguard.maven;

import com.google.gson.Gson;
import org.apache.maven.eventspy.AbstractEventSpy;
import org.apache.maven.execution.ExecutionEvent;
import org.apache.maven.project.MavenProject;
import org.apache.maven.plugin.PluginParameterExpressionEvaluator;
import org.codehaus.plexus.util.DirectoryScanner;
import org.codehaus.plexus.util.xml.Xpp3Dom;

import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import java.util.concurrent.ConcurrentHashMap;

public final class ModelSpy extends AbstractEventSpy {
    private final Map<String, Map<String, Object>> modules = new ConcurrentHashMap<>();
    private final Map<String, Set<String>> inputs = new ConcurrentHashMap<>();

    @Override
    public void onEvent(Object event) throws Exception {
        String output = System.getenv("DORNGUARD_MAVEN_MODEL");
        if (output == null || !(event instanceof ExecutionEvent)) {
            return;
        }
        ExecutionEvent execution = (ExecutionEvent) event;
        if (execution.getType() == ExecutionEvent.Type.MojoSucceeded
                && "org.apache.maven.plugins".equals(execution.getMojoExecution().getGroupId())
                && "maven-compiler-plugin".equals(execution.getMojoExecution().getArtifactId())
                && "compile".equals(execution.getMojoExecution().getGoal())) {
            Set<String> sources = compilerInputs(execution);
            inputs.computeIfAbsent(execution.getProject().getId(), key -> new TreeSet<>()).addAll(sources);
        } else if (execution.getType() == ExecutionEvent.Type.ProjectSucceeded) {
            MavenProject project = execution.getProject();
            Map<String, Object> module = new LinkedHashMap<>();
            module.put("path", project.getGroupId() + ":" + project.getArtifactId());
            module.put("name", project.getArtifactId());
            module.put("projectDir", project.getBasedir().getCanonicalPath());
            boolean sources = !"pom".equals(project.getPackaging());
            module.put("sourceDirs", sources ? absolutePaths(project.getCompileSourceRoots()) : Collections.emptyList());
            module.put("testSourceDirs", sources ? absolutePaths(project.getTestCompileSourceRoots()) : Collections.emptyList());
            module.put("compileSources", inputs.getOrDefault(project.getId(), Collections.emptySet()));
            module.put("kotlinSourceDirs", Collections.emptyList());
            module.put("classDirs", sources
                    ? absolutePaths(Collections.singletonList(project.getBuild().getOutputDirectory()))
                    : Collections.emptyList());
            module.put("classpath", sources ? absolutePaths(project.getCompileClasspathElements()) : Collections.emptyList());
            module.put("javaTarget", sources ? javaTarget(execution, project) : null);
            module.put("dependencies", Collections.emptyList());
            modules.put(project.getId(), module);
        } else if (execution.getType() == ExecutionEvent.Type.SessionEnded
                && !execution.getSession().getResult().hasExceptions()) {
            List<Map<String, Object>> ordered = new ArrayList<>();
            for (MavenProject project : execution.getSession().getProjects()) {
                Map<String, Object> module = modules.get(project.getId());
                if (module == null) {
                    return;
                }
                ordered.add(module);
            }
            Map<String, Object> model = new LinkedHashMap<>();
            model.put("schemaVersion", 1);
            model.put("modules", ordered);
            Path path = Paths.get(output);
            try (Writer writer = Files.newBufferedWriter(path, StandardCharsets.UTF_8)) {
                new Gson().toJson(model, writer);
            }
        }
    }

    private static Set<String> compilerInputs(ExecutionEvent execution) throws Exception {
        Xpp3Dom config = execution.getMojoExecution().getConfiguration();
        PluginParameterExpressionEvaluator evaluator = new PluginParameterExpressionEvaluator(execution.getSession(), execution.getMojoExecution());
        Xpp3Dom skip = config.getChild("skipMain");
        if (skip != null && Boolean.parseBoolean(String.valueOf(evaluator.evaluate(skip.getValue())))) {
            return Collections.emptySet();
        }
        List<String> includes = values(config.getChild("includes"), evaluator);
        List<String> excludes = values(config.getChild("excludes"), evaluator);
        Xpp3Dom roots = config.getChild("compileSourceRoots");
        List<String> directories = values(roots, evaluator);
        if (roots == null || roots.getChildCount() == 0 && directories.isEmpty()) {
            directories = execution.getProject().getCompileSourceRoots();
        }
        Set<String> files = new TreeSet<>();
        for (String directory : directories) {
            Path path = execution.getProject().getBasedir().toPath().resolve(directory);
            if (!Files.isDirectory(path)) {
                continue;
            }
            DirectoryScanner scanner = new DirectoryScanner();
            scanner.setBasedir(path.toFile());
            scanner.setIncludes(includes.isEmpty() ? new String[]{"**/*.java"} : includes.toArray(new String[0]));
            scanner.setExcludes(excludes.toArray(new String[0]));
            scanner.addDefaultExcludes();
            scanner.scan();
            for (String file : scanner.getIncludedFiles()) {
                if (file.endsWith(".java")) {
                    files.add(path.resolve(file).toFile().getCanonicalPath());
                }
            }
        }
        return files;
    }

    private static List<String> values(Xpp3Dom parameter, PluginParameterExpressionEvaluator evaluator) throws Exception {
        List<String> values = new ArrayList<>();
        if (parameter == null) {
            return values;
        }
        Xpp3Dom[] children = parameter.getChildren();
        for (Xpp3Dom entry : children.length == 0 ? new Xpp3Dom[]{parameter} : children) {
            String value = entry.getValue();
            if (value == null) {
                value = entry.getAttribute("default-value");
            }
            if (value == null) {
                continue;
            }
            Object evaluated = evaluator.evaluate(value);
            if (evaluated instanceof List) {
                for (Object part : (List<?>) evaluated) {
                    values.add((String) part);
                }
            } else if (evaluated instanceof String && !((String) evaluated).trim().isEmpty()) {
                for (String part : ((String) evaluated).split(",")) {
                    values.add(part.trim());
                }
            }
        }
        return values;
    }

    private static Integer javaTarget(ExecutionEvent event, MavenProject project) {
        Xpp3Dom config = project.getGoalConfiguration(
                "org.apache.maven.plugins", "maven-compiler-plugin", "default-compile", "compile");
        PluginParameterExpressionEvaluator evaluator = new PluginParameterExpressionEvaluator(event.getSession());
        String release = compilerParameter(config, evaluator, "release");
        String target = release != null ? release : compilerParameter(config, evaluator, "target");
        if (target == null || !target.matches("(?:1\\.)?[0-9]{1,4}")) {
            return null;
        }
        return Integer.valueOf(target.replaceFirst("^1\\.", ""));
    }

    private static String compilerParameter(Xpp3Dom config, PluginParameterExpressionEvaluator evaluator, String name) {
        Xpp3Dom child = config == null ? null : config.getChild(name);
        String value = child == null ? "${maven.compiler." + name + "}" : child.getValue();
        try {
            Object evaluated = evaluator.evaluate(value);
            String result = evaluated == null ? null : evaluated.toString().trim();
            return result == null || result.isEmpty() || result.equals("${maven.compiler." + name + "}") ? null : result;
        } catch (Exception ignored) {
            return "";
        }
    }

    private static List<String> absolutePaths(List<String> paths) throws Exception {
        List<String> absolute = new ArrayList<>();
        for (String path : paths) {
            absolute.add(Paths.get(path).toFile().getCanonicalPath());
        }
        return absolute;
    }
}
