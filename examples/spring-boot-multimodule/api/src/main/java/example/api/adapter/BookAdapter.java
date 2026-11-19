package example.api.adapter;

import example.api.domain.Book;
import example.common.Titles;

public final class BookAdapter {
    public Book create(String title) {
        return new Book(Titles.normalize(title));
    }
}
