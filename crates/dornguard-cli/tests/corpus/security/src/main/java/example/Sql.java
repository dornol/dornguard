package example;

class Sql {
    void flagged(JdbcTemplate jdbc, Statement st, EntityManager em, MachineDao dao, String id, String table) {
        jdbc.query("select * from t where id = " + id, mapper); // expect: security.sql-injection
        st.executeQuery("select * from t where n = '" + id + "'"); // expect: security.sql-injection
        em.createNativeQuery("select " + id + " from t"); // expect: security.sql-injection
        jdbc.update(String.format("delete from t where id = %s", id)); // expect: security.sql-injection
        dao.update("update machine set name = '" + id + "'"); // expect: security.sql-injection
        dao.execute("DROP TABLE " + table); // expect: security.sql-injection
        dao.execute("KILL " + id); // expect: security.sql-injection
    }

    void clean(JdbcTemplate jdbc, Statement st, EntityManager em, Signature signature, Mac mac,
            MessageDigest digest, Cache cache, String id, byte[] data) throws Exception {
        jdbc.queryForObject("select x from t where id = ?", String.class, id);
        jdbc.update("update t set a = ? where id = ?", id, id);
        st.executeQuery("select 1 from " + "dual");
        em.createQuery("select u from User u where u.id = :id");
        // Signing and hashing APIs share the method name `update` with JDBC.
        signature.update(("id=" + id).getBytes());
        mac.update(("id=" + id).getBytes());
        digest.update(("id=" + id).getBytes());
        // Cache and map style APIs concatenate keys, not SQL.
        cache.update("user:" + id);
        cache.execute(id + id);
    }
}
