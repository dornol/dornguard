package io.dornguard.runner;

import com.tngtech.archunit.thirdparty.org.objectweb.asm.AnnotationVisitor;
import com.tngtech.archunit.thirdparty.org.objectweb.asm.ClassReader;
import com.tngtech.archunit.thirdparty.org.objectweb.asm.ClassVisitor;
import com.tngtech.archunit.thirdparty.org.objectweb.asm.Opcodes;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Pattern;

final class SourceIndex {
    private static final Pattern PACKAGE = Pattern.compile(
            "(?m)^\\h*package\\h+((?:[\\p{L}_][\\p{L}\\p{N}_]*|`[^`\\r\\n]+`)"
                    + "(?:\\h*\\.\\h*(?:[\\p{L}_][\\p{L}\\p{N}_]*|`[^`\\r\\n]+`))*)");
    private static final Pattern COMMENTS_AND_STRINGS = Pattern.compile(
            "(?s)/\\*|//[^\\r\\n]*|\"\"\".*?\"\"\"|\"(?:\\\\.|[^\"\\\\])*\"|'(?:\\\\.|[^'\\\\])'");

    private final Map<String, List<Path>> files = new HashMap<>();
    private final Map<Path, String> contents = new HashMap<>();
    private final List<Path> classDirs;

    SourceIndex(List<String> sources, List<Path> classDirs) {
        this.classDirs = classDirs;
        for (String source : sources) {
            Path path = Path.of(source);
            files.computeIfAbsent(path.getFileName().toString(), unused -> new ArrayList<>()).add(path);
        }
    }

    Origin locate(String className, String packageName) throws IOException {
        ClassSource source = classSource(className);
        if (source == null || source.file == null) {
            return new Origin(null, source != null && source.kotlin, 0);
        }
        boolean kotlin = source.kotlin || source.file.endsWith(".kt");
        List<Path> candidates = files.getOrDefault(source.file, List.of());
        Path unique = null;
        for (Path candidate : candidates) {
            if ((!kotlin && candidates.size() == 1) || packageName.equals(declaredPackage(read(candidate)))) {
                if (unique != null) {
                    return new Origin(null, kotlin, 0);
                }
                unique = candidate;
            }
        }
        if (unique == null) {
            return new Origin(null, kotlin, 0);
        }

        // Kotlin inline expansions can contain line numbers belonging to another source.
        long lines = kotlin ? (source.hasDebugMapping ? 0 : read(unique).lines().count()) : Long.MAX_VALUE;
        return new Origin(unique.toString(), kotlin, lines);
    }

    private String read(Path path) throws IOException {
        String cached = contents.get(path);
        if (cached == null) {
            cached = Files.readString(path);
            contents.put(path, cached);
        }
        return cached;
    }

    private ClassSource classSource(String className) throws IOException {
        String relative = className.replace('.', '/') + ".class";
        for (Path directory : classDirs) {
            Path file = directory.resolve(relative);
            if (Files.isRegularFile(file)) {
                ClassSource source = new ClassSource();
                new ClassReader(Files.readAllBytes(file)).accept(source, ClassReader.SKIP_CODE | ClassReader.SKIP_FRAMES);
                return source;
            }
        }
        return null;
    }

    private static String declaredPackage(String text) {
        var code = new StringBuilder(text);
        var nonCode = COMMENTS_AND_STRINGS.matcher(text);
        int start = 0;
        while (nonCode.find(start)) {
            start = nonCode.end();
            if (nonCode.group().equals("/*")) {
                start = endOfComment(text, start);
            }
            for (int index = nonCode.start(); index < start; index++) {
                char value = text.charAt(index);
                if (value != '\r' && value != '\n') {
                    code.setCharAt(index, ' ');
                }
            }
        }
        var declaration = PACKAGE.matcher(code);
        if (!declaration.find()) {
            return "";
        }
        String name = declaration.group(1).replaceAll("[\\s`]", "");
        return declaration.find() ? null : name;
    }

    private static int endOfComment(String text, int start) {
        int depth = 1;
        for (int index = start; index + 1 < text.length(); index++) {
            if (text.startsWith("/*", index)) {
                depth++;
                index++;
            } else if (text.startsWith("*/", index)) {
                if (--depth == 0) {
                    return index + 2;
                }
                index++;
            }
        }
        return text.length();
    }

    record Origin(String path, boolean kotlin, long lines) {
        Integer line(int number) {
            return number > 0 && number <= lines ? number : null;
        }
    }

    private static final class ClassSource extends ClassVisitor {
        private String file;
        private boolean hasDebugMapping;
        private boolean kotlin;

        private ClassSource() {
            super(Opcodes.ASM9);
        }

        @Override
        public void visitSource(String source, String debug) {
            file = source;
            hasDebugMapping = debug != null && !debug.isEmpty();
        }

        @Override
        public AnnotationVisitor visitAnnotation(String descriptor, boolean visible) {
            kotlin |= descriptor.equals("Lkotlin/Metadata;");
            return null;
        }
    }
}
