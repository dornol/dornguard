package example;

class Crypto {
    void flagged() throws Exception {
        MessageDigest.getInstance("MD5"); // expect: security.weak-crypto
        MessageDigest.getInstance("SHA-1"); // expect: security.weak-crypto
        DigestUtils.sha1Hex(data); // expect: security.weak-crypto
        Cipher.getInstance("AES"); // expect: security.weak-crypto
        Cipher.getInstance("AES/ECB/PKCS5Padding"); // expect: security.weak-crypto
        Cipher.getInstance("DES/CBC/PKCS5Padding"); // expect: security.weak-crypto
    }

    void clean() throws Exception {
        MessageDigest.getInstance("SHA-256");
        DigestUtils.sha256Hex(data);
        Cipher.getInstance("AES/GCM/NoPadding");
    }
}
