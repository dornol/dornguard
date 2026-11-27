package example;

import java.io.Serializable;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

class Unused implements Serializable {
    private static final long serialVersionUID = 1L;
    private String forgotten; // expect: java.unused-private-field
    private final String label;

    Unused(String label) {
        this.label = label;
    }

    private void leftover() { // expect: java.unused-private-method
    }

    private String format() {
        return label.trim();
    }

    String describe() {
        String copy = label; // expect: java.unused-local-variable
        // PMD skips a variable named ignored... or unused... by convention.
        String unusedCopy = label;
        return format();
    }

    List<String> names(boolean known) {
        if (!known) {
            return null; // expect: java.return-null-collection
        }
        return new ArrayList<>();
    }

    String[] tags(boolean known) {
        if (!known) {
            return null; // expect: java.return-null-collection
        }
        return new String[0];
    }

    Map<String, String> settings() {
        return Collections.emptyMap();
    }

    String nickname() {
        return null;
    }
}
