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

const EXEMPT: &str = r#"class Exempt {
    void signing(Signature signature, Mac mac, String name) throws Exception {
        signature.update(("id=" + name).getBytes());
        mac.update(("id=" + name).getBytes());
    }

    void tls(boolean verifySsl, Builder builder) {
        if (!verifySsl) {
            builder.hostnameVerifier(NoopHostnameVerifier.INSTANCE);
        }
    }

    SecurityFilterChain stateless(HttpSecurity http) throws Exception {
        http.csrf(AbstractHttpConfigurer::disable)
            .sessionManagement(s -> s.sessionCreationPolicy(SessionCreationPolicy.STATELESS));
        return http.build();
    }
}
"#;

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn security_profile_skips_stateless_chains_toggled_tls_hashing_calls_and_tests() {
    let project = project(EXEMPT);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [security]\n",
    )
    .unwrap();
    let tests = root.join("src/test/java");
    fs::create_dir_all(&tests).unwrap();
    fs::write(tests.join("Legacy.java"), SOURCE).unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["diagnostics"], serde_json::json!([]), "{result}");
}

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn security_profile_keeps_reporting_lookalikes_of_the_exempt_patterns() {
    let source = r#"class Lookalike {
    void sql(MachineDao machineDao, Builder builder, boolean ready) {
        machineDao.update("update machine set name = '" + name + "'");
        machineDao.execute("DROP TABLE " + table);
        if (ready) {
            builder.hostnameVerifier(NoopHostnameVerifier.INSTANCE);
        }
    }
}
"#;
    let project = project(source);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [security]\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    let lines = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["line"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines, [3, 4, 6], "{result}");
}

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn security_findings_carry_confidence_and_a_review_hint() {
    let project = project(SOURCE);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [security]\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    let confidence = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["confidence"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(confidence, ["medium"].into(), "{result}");

    let text = std::process::Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["check", "--format", "agent"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("Review: This rule infers intent"), "{text}");
}
