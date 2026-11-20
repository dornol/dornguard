package io.dornguard.runner;

import com.tngtech.archunit.thirdparty.org.objectweb.asm.AnnotationVisitor;
import com.tngtech.archunit.thirdparty.org.objectweb.asm.ClassReader;
import com.tngtech.archunit.thirdparty.org.objectweb.asm.ClassVisitor;
import com.tngtech.archunit.thirdparty.org.objectweb.asm.Opcodes;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.FileTime;
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

    private static final Pattern JAVA_WITHOUT_TYPES = Pattern.compile(
            "(?:\\s|;|(?:package|import)\\s+[^;{}]+;)*");

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
        if (source == null) {
            return new Origin(null, false, 0, null);
        }
        boolean kotlin = source.kotlin || source.file != null && source.file.endsWith(".kt");
        String file = source.file;
        boolean inferred = file == null && !kotlin;
        if (inferred) {
            String simpleName = className.substring(className.lastIndexOf('.') + 1);
            file = simpleName.replaceFirst("\\$.*$", "") + ".java";
        }
        List<Path> candidates = file == null ? List.of() : files.getOrDefault(file, List.of());
        Path unique = null;
        for (Path candidate : candidates) {
            if ((!inferred && !kotlin && candidates.size() == 1) || packageName.equals(declaredPackage(read(candidate), kotlin))) {
                if (unique != null) {
                    return new Origin(null, kotlin, 0, null);
                }
                unique = candidate;
            }
        }
        if (unique == null) {
            return new Origin(null, kotlin, 0, null);
        }

        // Kotlin inline expansions can contain line numbers belonging to another source.
        long lines = inferred ? 0 : kotlin ? (source.hasDebugMapping ? 0 : read(unique).lines().count()) : Long.MAX_VALUE;
        return new Origin(unique.toString(), kotlin, lines, source.time);
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
                source.time = Files.getLastModifiedTime(file);
                new ClassReader(Files.readAllBytes(file)).accept(source, ClassReader.SKIP_CODE | ClassReader.SKIP_FRAMES);
                return source;
            }
        }
        return null;
    }

    boolean expectsJavaClass(Path path) throws IOException {
        String name = path.getFileName().toString();
        return name.endsWith(".java") && !name.equals("module-info.java") && !name.equals("package-info.java")
                && !JAVA_WITHOUT_TYPES.matcher(codeOnly(read(path), false)).matches();
    }

    private static CharSequence codeOnly(String text, boolean nestedComments) {
        var code = new StringBuilder(text);
        var nonCode = COMMENTS_AND_STRINGS.matcher(text);
        int start = 0;
        while (nonCode.find(start)) {
            start = nonCode.end();
            if (nonCode.group().equals("/*")) {
                if (nestedComments) {
                    start = endOfComment(text, start);
                } else {
                    int end = text.indexOf("*/", start);
                    start = end == -1 ? text.length() : end + 2;
                }
            }
            for (int index = nonCode.start(); index < start; index++) {
                char value = text.charAt(index);
                if (value != '\r' && value != '\n') {
                    code.setCharAt(index, ' ');
                }
            }
        }
        return code;
    }

    private static String declaredPackage(String text, boolean nestedComments) {
        var declaration = PACKAGE.matcher(codeOnly(text, nestedComments));
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

    record Origin(String path, boolean kotlin, long lines, FileTime classTime) {
        Integer line(int number) {
            return number > 0 && number <= lines ? number : null;
        }
    }

    private static final class ClassSource extends ClassVisitor {
        private String file;
        private boolean hasDebugMapping;
        private boolean kotlin;
        private FileTime time;

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
