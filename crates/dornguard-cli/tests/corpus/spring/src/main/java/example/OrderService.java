package example;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;

@Service
class OrderService {
    @Autowired // expect: spring.field-injection
    private OrderRepository orders;

    @org.springframework.beans.factory.annotation.Autowired // expect: spring.field-injection, java.fully-qualified-type
    private PaymentClient payments;

    // Configuration values are not injected collaborators.
    @Value("${orders.limit}")
    private int limit;

    private final AuditLog audit;

    OrderService(AuditLog audit) {
        this.audit = audit;
    }
}
