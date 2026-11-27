package example;

import java.util.Optional;
import java.util.function.Supplier;

class Optionals {
    private Optional<String> cached; // expect: java.optional-misuse
    private final Optional<Integer> limit = Optional.empty(); // expect: java.optional-misuse
    private String nickname;

    Optionals(Optional<String> name) { // expect: java.optional-misuse
    }

    void rename(String id, Optional<String> name) { // expect: java.optional-misuse
    }

    Optional<String> find(String id) {
        if (id.isEmpty()) {
            return null; // expect: java.optional-misuse
        }
        return Optional.of(id);
    }

    // Optional is a return type and a local variable type by design.
    Optional<String> nickname() {
        Optional<String> local = Optional.ofNullable(nickname);
        return local;
    }

    // null returned from a lambda inside an Optional method is the lambda's result.
    Optional<Supplier<String>> supplier() {
        return Optional.of(() -> {
            return null;
        });
    }

    String plain() {
        return null;
    }
}
