package example;

// Large, branchy, reads other classes' fields and its methods share no fields.
class OrderManager { // expect: java.god-class
    static class Customer {
        String name;
        boolean vip;
    }

    static class Invoice {
        int amount;
        boolean paid;
        int lines;
    }

    private int f0;
    private int f1;
    private int f2;
    private int f3;
    private int f4;
    private int f5;
    private int f6;
    private int f7;
    private int f8;
    private int f9;
    private int f10;
    private int f11;

    int step0(Customer customer, Invoice invoice) {
        int total = f0;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step1(Customer customer, Invoice invoice) {
        int total = f1;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step2(Customer customer, Invoice invoice) {
        int total = f2;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step3(Customer customer, Invoice invoice) {
        int total = f3;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step4(Customer customer, Invoice invoice) {
        int total = f4;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step5(Customer customer, Invoice invoice) {
        int total = f5;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step6(Customer customer, Invoice invoice) {
        int total = f6;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step7(Customer customer, Invoice invoice) {
        int total = f7;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step8(Customer customer, Invoice invoice) {
        int total = f8;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step9(Customer customer, Invoice invoice) {
        int total = f9;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step10(Customer customer, Invoice invoice) {
        int total = f10;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }

    int step11(Customer customer, Invoice invoice) {
        int total = f11;
        if (customer.name == null) {
            total += 1;
        } else if (customer.vip) {
            total += 2;
        }
        if (invoice.amount > 100 && invoice.paid) {
            total += 3;
        }
        for (int n = 0; n < invoice.lines; n++) {
            total += n;
        }
        return total;
    }
}
