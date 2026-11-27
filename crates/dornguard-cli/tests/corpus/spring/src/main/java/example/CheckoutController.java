package example;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Controller;
import org.springframework.transaction.annotation.Transactional;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.ModelAttribute;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.PutMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;
import org.springframework.web.client.RestClient;
import org.springframework.web.client.RestTemplate;

@RestController
class CheckoutController {
    private final OrderService orders;

    @Autowired // expect: spring.redundant-autowired-constructor
    CheckoutController(OrderService orders) {
        this.orders = orders;
    }

    @PostMapping("/orders")
    @Transactional
    String place(@RequestBody Order order) { // expect: spring.transactional-in-controller, spring.jpa-entity-request-body
        return "placed";
    }

    @PutMapping("/orders/{id}")
    String update(@PathVariable Long id, @RequestBody OrderView view) {
        return "updated";
    }

    @GetMapping("/orders/{id}")
    String show(@PathVariable("id") Order order) {
        return "order";
    }

    String rates() {
        RestTemplate template = new RestTemplate(); // expect: spring.http-client-without-timeout
        RestClient client = RestClient.create(); // expect: spring.http-client-without-timeout
        return template.toString() + client;
    }
}

@Transactional
@Controller
class FormController { // expect: spring.transactional-in-controller
    @PostMapping("/customers")
    String save(@ModelAttribute Customer customer) { // expect: spring.jpa-entity-request-body
        return "redirect:/customers";
    }

    @PostMapping("/customers/import")
    String bulk(Customer customer) { // expect: spring.jpa-entity-request-body
        return "redirect:/customers";
    }
}

// With several constructors, @Autowired picks the one Spring uses.
@Controller
class ReportController {
    private final OrderService orders;

    ReportController() {
        this(null);
    }

    @Autowired
    ReportController(OrderService orders) {
        this.orders = orders;
    }

    String client() {
        RestTemplate template = new RestTemplate();
        template.setRequestFactory(null);
        return template.toString();
    }
}
