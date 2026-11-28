package example;

import java.util.List;
import java.util.Set;
import org.jspecify.annotations.Nullable;

class DeclaredResults {
    private @Nullable Set<String> names;
    private String @Nullable [] options;

    // A return type whose outermost type is @Nullable declares null as a result.
    @Nullable Set<String> nullableSet() {
        return names == null ? null : Set.copyOf(names);
    }

    String @Nullable [] nullableArray() {
        return options != null ? options.clone() : null;
    }

    // Only the elements are nullable here; the array itself is not.
    @Nullable String[] nullableElements() {
        return null; // expect: java.return-null-collection
    }

    List<String> plain() {
        return null; // expect: java.return-null-collection
    }

    static final class CallbackFailure extends RuntimeException {
        private final RuntimeException original;

        CallbackFailure(RuntimeException original) {
            this.original = original;
        }

        RuntimeException original() {
            return original;
        }
    }

    void unwrap(Runnable task) {
        try {
            task.run();
        } catch (CallbackFailure e) {
            throw e.original();
        }
    }

    void unwrapCause(Runnable task) {
        try {
            task.run();
        } catch (IllegalStateException e) {
            throw (RuntimeException) e.getCause();
        }
    }

    void message(Runnable task) {
        try {
            task.run();
        } catch (IllegalStateException e) {
            throw new IllegalArgumentException(e.getMessage()); // expect: java.preserve-stack-trace
        }
    }

    static String encode(String text) {
        return text.isEmpty() ? "-" : text;
    }

    // Whether encode returned the instance it was given.
    boolean unchanged(String text) {
        String encoded = encode(text);
        return encoded == text;
    }

    boolean compare(String left, String right) {
        return left == right; // expect: java.string-reference-equality
    }
}
