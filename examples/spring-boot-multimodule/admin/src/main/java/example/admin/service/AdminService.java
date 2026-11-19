package example.admin.service;

import example.common.Titles;

public final class AdminService {
    public boolean ready() {
        return !Titles.normalize("DornGuard").isEmpty();
    }
}
