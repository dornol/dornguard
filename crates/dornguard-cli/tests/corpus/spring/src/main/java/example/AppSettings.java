package example;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Component;

@Component
class AppSettings {
    // Spring skips static members, so these keep their initial values.
    @Value("${app.name}") // expect: spring.static-member-injection
    private static String name;

    @Value("${app.port:8080}") // expect: spring.static-member-injection
    static int port = 8080;

    private static String region;

    @Value("${app.region}") // expect: spring.static-member-injection
    public static void setRegion(String value) {
        region = value;
    }

    @Autowired // expect: spring.static-member-injection
    static void register(OrderRepository orders) {
    }

    // spring.field-injection reports an @Autowired field once, static or not.
    @Autowired // expect: spring.field-injection
    private static Mailer mailer;

    // Instance members are injected.
    @Value("${app.timeout}")
    private int timeout;

    private static String zone;

    @Value("${app.zone}")
    void setZone(String value) {
        zone = value;
    }

    // A constant without injection is fine.
    static final String PREFIX = "app";
}
