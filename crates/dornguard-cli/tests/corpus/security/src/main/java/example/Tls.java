package example;

class Tls {
    void flagged(HttpsURLConnection connection, Builder builder, boolean ready) {
        connection.setHostnameVerifier((host, session) -> true); // expect: security.disabled-tls-verification
        TrustManager trustAll = new X509TrustManager() { // expect: security.disabled-tls-verification
            public void checkClientTrusted(X509Certificate[] chain, String type) {}
            public void checkServerTrusted(X509Certificate[] chain, String type) {}
            public X509Certificate[] getAcceptedIssuers() { return null; }
        };
        // A condition unrelated to verification does not make the bypass safe.
        if (ready) {
            builder.hostnameVerifier(NoopHostnameVerifier.INSTANCE); // expect: security.disabled-tls-verification
        }
    }

    void clean(HttpsURLConnection connection, Builder builder, boolean verifySsl, boolean insecure) {
        connection.setHostnameVerifier((host, session) -> host.equals("example.com"));
        // An explicit option that defaults to verification chooses the bypass.
        if (!verifySsl) {
            builder.hostnameVerifier(NoopHostnameVerifier.INSTANCE);
        }
        builder.hostnameVerifier(insecure ? NoopHostnameVerifier.INSTANCE : defaultVerifier);
    }
}
