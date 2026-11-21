package example;

// Test sources are skipped by default, so nothing here is reported.
class LegacyTest {
    void fixtures(JdbcTemplate jdbc, String id) throws Exception {
        jdbc.query("select * from t where id = " + id, mapper);
        MessageDigest.getInstance("MD5");
    }
}
