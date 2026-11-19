package example.admin.controller;

import example.admin.service.AdminService;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
public final class AdminController {
    private final AdminService service = new AdminService();

    @GetMapping("/admin/ready")
    public boolean ready() {
        return service.ready();
    }
}
