package example;

class Csrf {
    SecurityFilterChain session(HttpSecurity http) throws Exception {
        http.csrf().disable(); // expect: security.csrf-disabled
        return http.build();
    }

    SecurityFilterChain sessionWithMethodReference(HttpSecurity http) throws Exception {
        http.csrf(AbstractHttpConfigurer::disable); // expect: security.csrf-disabled
        return http.build();
    }

    // A stateless token API has no session cookie to forge.
    SecurityFilterChain stateless(HttpSecurity http) throws Exception {
        http.csrf(AbstractHttpConfigurer::disable)
            .sessionManagement(s -> s.sessionCreationPolicy(SessionCreationPolicy.STATELESS));
        return http.build();
    }
}
