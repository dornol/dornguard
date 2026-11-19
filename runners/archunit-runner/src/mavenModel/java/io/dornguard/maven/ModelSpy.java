package io.dornguard.maven;

import com.google.gson.Gson;
import org.apache.maven.eventspy.AbstractEventSpy;
import org.apache.maven.execution.ExecutionEvent;
import org.apache.maven.project.MavenProject;

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
import java.util.concurrent.ConcurrentHashMap;

public final class ModelSpy extends AbstractEventSpy {
    private final Map<String, Map<String, Object>> modules = new ConcurrentHashMap<>();

    @Override
    public void onEvent(Object event) throws Exception {
        String output = System.getenv("DORNGUARD_MAVEN_MODEL");
        if (output == null || !(event instanceof ExecutionEvent)) {
            return;
        }
        ExecutionEvent execution = (ExecutionEvent) event;
        if (execution.getType() == ExecutionEvent.Type.ProjectSucceeded) {
            MavenProject project = execution.getProject();
            Map<String, Object> module = new LinkedHashMap<>();
            module.put("path", project.getGroupId() + ":" + project.getArtifactId());
            module.put("name", project.getArtifactId());
            module.put("projectDir", project.getBasedir().getCanonicalPath());
            boolean sources = !"pom".equals(project.getPackaging());
            module.put("sourceDirs", sources ? absolutePaths(project.getCompileSourceRoots()) : Collections.emptyList());
            module.put("kotlinSourceDirs", Collections.emptyList());
            module.put("classDirs", sources
                    ? absolutePaths(Collections.singletonList(project.getBuild().getOutputDirectory()))
                    : Collections.emptyList());
            module.put("classpath", sources ? absolutePaths(project.getCompileClasspathElements()) : Collections.emptyList());
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

    private static List<String> absolutePaths(List<String> paths) throws Exception {
        List<String> absolute = new ArrayList<>();
        for (String path : paths) {
            absolute.add(Paths.get(path).toFile().getCanonicalPath());
        }
        return absolute;
    }
}
