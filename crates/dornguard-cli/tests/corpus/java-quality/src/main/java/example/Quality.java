package example;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

class Quality {
    private static final Logger log = LoggerFactory.getLogger(Quality.class);
    private static Quality instance;
    private static volatile Quality published;

    void logs(String user, Exception error) {
        log.info("user {} signed in from {}", user); // expect: java.log-format-arguments
        log.info("user {}", user, user); // expect: java.log-format-arguments
        log.info("user {}", user);
        // A trailing exception needs no placeholder.
        log.error("sign-in failed for {}", user, error);
        log.warn("no placeholders");
    }

    boolean sameName(String name, String other) {
        if (name == "admin") { // expect: java.string-reference-equality
            return true;
        }
        if (name != other) { // expect: java.string-reference-equality
            return false;
        }
        // A null check is an identity comparison by design.
        return name == null || name.equals(other);
    }

    void closeQuietly() throws Exception {
        try {
            run();
        } finally { // expect: java.throw-from-finally
            throw new IllegalStateException("cleanup failed");
        }
    }

    void cleanUp() {
        try {
            run();
        } finally {
            release();
        }
    }

    static Quality get() { // expect: java.double-checked-locking
        if (instance == null) {
            synchronized (Quality.class) {
                if (instance == null) {
                    instance = new Quality();
                }
            }
        }
        return instance;
    }

    // A volatile field makes double-checked locking safe.
    static Quality getPublished() {
        if (published == null) {
            synchronized (Quality.class) {
                if (published == null) {
                    published = new Quality();
                }
            }
        }
        return published;
    }

    void run() {}

    void release() {}
}
