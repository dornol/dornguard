package example;

import java.util.*; // expect: java.wildcard-import
import static java.util.Objects.*; // expect: java.wildcard-import
import java.util.concurrent.TimeUnit;

class Imports {
    // "import java.io.*;" in a comment or string is not an import.
    String text = "import java.io.*;";

    List<String> names() {
        requireNonNull(TimeUnit.SECONDS);
        return new ArrayList<>();
    }
}
