package example;

import java.text.SimpleDateFormat;
import java.time.format.DateTimeFormatter;
import java.util.Locale;
import org.apache.commons.lang3.time.DateFormatUtils;
import org.apache.commons.lang3.time.FastDateFormat;

class Dates {
    // YYYY is the week-based year: 2026-12-28 formats as 2027-12-28.
    static final DateTimeFormatter WEEK_YEAR = DateTimeFormatter.ofPattern("YYYY-MM-dd"); // expect: java.date-pattern-typo
    // DD after MM is the day of the year: 2026-02-01 formats as 2026-02-32.
    static final DateTimeFormatter DAY_OF_YEAR = DateTimeFormatter.ofPattern("yyyy-MM-DD", Locale.ROOT); // expect: java.date-pattern-typo
    final SimpleDateFormat legacy = new SimpleDateFormat("dd/MM/YYYY"); // expect: java.date-pattern-typo
    static final FastDateFormat FAST = FastDateFormat.getInstance("YYYYMMdd"); // expect: java.date-pattern-typo

    // The calendar year and day of month are correct.
    static final DateTimeFormatter ISO = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss");
    static final DateTimeFormatter LOCAL = DateTimeFormatter.ofPattern("uuuu-MM-dd", Locale.ROOT);
    // A week-based year belongs with a week of the year.
    static final DateTimeFormatter ISO_WEEK = DateTimeFormatter.ofPattern("YYYY-'W'ww-e");
    // The day of the year without a month is intended.
    static final DateTimeFormatter ORDINAL = DateTimeFormatter.ofPattern("yyyy-DDD");
    final SimpleDateFormat compact = new SimpleDateFormat("yyyyMMdd", Locale.ROOT);

    String format(java.util.Date date) { // expect: java.fully-qualified-type
        legacy.applyPattern("MM/DD/yyyy"); // expect: java.date-pattern-typo
        compact.applyPattern("MM/dd/yyyy");
        return DateFormatUtils.format(date, "YYYY-MM-dd"); // expect: java.date-pattern-typo
    }
}
