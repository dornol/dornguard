package example;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;
import org.springframework.web.bind.annotation.RestController;

// A class-level mapping only shares the path prefix.
@RestController
@RequestMapping("/orders")
class OrderController {
    @RequestMapping("/cancel")
    String cancel() { // expect: spring.request-mapping-without-method
        return "cancelled";
    }

    @RequestMapping
    String all() { // expect: spring.request-mapping-without-method
        return "all";
    }

    @RequestMapping(value = "/export", produces = "text/csv")
    String export() { // expect: spring.request-mapping-without-method
        return "";
    }

    @RequestMapping(value = "/archive", method = RequestMethod.POST)
    String archive() {
        return "archived";
    }

    @RequestMapping(method = {RequestMethod.GET, RequestMethod.HEAD}, value = "/status")
    String status() {
        return "ok";
    }

    @GetMapping("/{id}")
    String show() {
        return "order";
    }

    @PostMapping
    String create() {
        return "created";
    }
}
