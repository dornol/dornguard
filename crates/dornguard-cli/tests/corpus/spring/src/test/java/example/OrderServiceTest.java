package example;

// Test fixtures commonly inject with field annotations; they are skipped by default.
class OrderServiceTest {
    @Autowired
    private OrderService service;
}
