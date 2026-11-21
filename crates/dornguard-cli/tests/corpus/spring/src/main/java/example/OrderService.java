package example;

import jakarta.annotation.Resource;
import jakarta.inject.Inject;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.beans.factory.annotation.Qualifier;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;

@Service
class OrderService {
    @Autowired // expect: spring.field-injection
    private OrderRepository orders;

    @org.springframework.beans.factory.annotation.Autowired // expect: spring.field-injection, java.fully-qualified-type
    private PaymentClient payments;

    // Annotation arguments, other annotations and JSR-330/250 forms are field injection too.
    @Autowired(required = false) // expect: spring.field-injection
    private MetricsClient metrics;

    @Autowired // expect: spring.field-injection
    @Qualifier("primary")
    private AuditLog primaryAudit;

    @Inject // expect: spring.field-injection
    private TaxService taxes;

    @Resource(name = "mailer") // expect: spring.field-injection
    protected Mailer mailer;

    // Setter injection and configuration values are not field injection.
    @Autowired
    void setClock(Clock clock) {}

    @Value("${orders.limit}")
    private int limit;

    private final AuditLog audit;

    OrderService(AuditLog audit) {
        this.audit = audit;
    }
}
