package example;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

class Design {
    private final HashMap<String, Integer> counts = new HashMap<>(); // expect: java.loose-coupling
    private final Map<String, Integer> totals = new HashMap<>();

    ArrayList<String> names() { // expect: java.loose-coupling
        return new ArrayList<>();
    }

    List<String> labels() {
        return new ArrayList<>();
    }

    void register(String a, String b, String c, String d, String e, String f, String g, String h) { // expect: java.excessive-parameters
    }

    void configure(String a, String b, String c, String d, String e, String f, String g) {
    }

    int score(int[] values, boolean strict, boolean lenient) { // expect: java.cognitive-complexity
        int total = 0;
        for (int value : values) {
            if (value > 0) {
                if (strict) {
                    for (int i = 0; i < value; i++) {
                        if (i % 2 == 0 && lenient || i % 3 == 0) {
                            total++;
                        } else if (i % 5 == 0) {
                            total--;
                        }
                    }
                } else if (lenient) {
                    while (total < value) {
                        total += value > 10 ? 2 : 1;
                    }
                }
            } else {
                switch (value) {
                    case -1:
                        total -= 1;
                        break;
                    default:
                        total = strict ? 0 : total;
                }
            }
        }
        return total;
    }

    int simple(int value) {
        return value > 0 ? value : -value;
    }
}
