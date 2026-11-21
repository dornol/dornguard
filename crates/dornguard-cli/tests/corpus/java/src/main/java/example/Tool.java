package example;

class Tool {
    // A command-line entry point writes to standard output by design.
    public static void main(String[] args) {
        System.out.println("usage: tool FILE");
        Runnable report = () -> System.out.println("done");
    }

    public static void main(String... args) {
        System.out.println("varargs entry point");
    }

    // Other methods use the project logger, even when main calls them.
    static void helper() {
        System.out.println("progress"); // expect: java.stdout-usage
    }

    void service() {
        new Exception().printStackTrace(); // expect: java.print-stack-trace
    }
}
