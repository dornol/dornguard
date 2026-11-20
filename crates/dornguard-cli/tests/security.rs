#![allow(clippy::unwrap_used)]
use std::fs;

mod common;
use common::{check, project, report};

const SOURCE: &str = r#"class Example {
    void sql(JdbcTemplate jdbc, Statement st, EntityManager em, String id) {
        jdbc.query("select * from t where id = " + id, mapper);
        st.executeQuery("select * from t where n = '" + id + "'");
        em.createNativeQuery("select " + id + " from t");
        jdbc.update(String.format("delete from t where id = %s", id));
        jdbc.queryForObject("select x from t where id = ?", String.class, id);
        st.executeQuery("select 1 from " + "dual");
        em.createQuery("select u from User u where u.id = :id");
        map.execute(id + id);
    }

    void crypto() throws Exception {
        MessageDigest.getInstance("MD5");
        MessageDigest.getInstance("SHA-256");
        DigestUtils.sha1Hex(data);
        Cipher.getInstance("AES");
        Cipher.getInstance("AES/ECB/PKCS5Padding");
        Cipher.getInstance("AES/GCM/NoPadding");
        Cipher.getInstance("DES/CBC/PKCS5Padding");
    }

    void tls(HttpsURLConnection connection) {
        connection.setHostnameVerifier((host, session) -> true);
        connection.setHostnameVerifier((host, session) -> host.equals("example.com"));
        TrustManager trustAll = new X509TrustManager() {
            public void checkClientTrusted(X509Certificate[] chain, String type) {}
            public void checkServerTrusted(X509Certificate[] chain, String type) {}
            public X509Certificate[] getAcceptedIssuers() { return null; }
        };
    }

    void csrf(HttpSecurity http) throws Exception {
        http.csrf().disable();
        http.csrf(AbstractHttpConfigurer::disable);
        http.csrf(csrf -> csrf.ignoringRequestMatchers("/webhook"));
        http.authorizeHttpRequests(requests -> requests.anyRequest().authenticated());
    }
}
"#;

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn security_profile_flags_injection_weak_crypto_tls_and_csrf_patterns() {
    let project = project(SOURCE);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [security]\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    let mut findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap(),
                finding["severity"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort_unstable();
    assert_eq!(
        findings,
        [
            (3, "security.sql-injection", "error"),
            (4, "security.sql-injection", "error"),
            (5, "security.sql-injection", "error"),
            (6, "security.sql-injection", "error"),
            (14, "security.weak-crypto", "warning"),
            (16, "security.weak-crypto", "warning"),
            (17, "security.weak-crypto", "warning"),
            (18, "security.weak-crypto", "warning"),
            (20, "security.weak-crypto", "warning"),
            (24, "security.disabled-tls-verification", "error"),
            (26, "security.disabled-tls-verification", "error"),
            (34, "security.csrf-disabled", "warning"),
            (35, "security.csrf-disabled", "warning"),
        ],
        "{result}"
    );
}
