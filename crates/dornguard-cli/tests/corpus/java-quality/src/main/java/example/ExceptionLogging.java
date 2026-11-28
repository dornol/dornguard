package example;

import com.acme.AcmeError;
import com.acme.AcmeException;
import com.acme.Request;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

// The com.acme types are not on the classpath, as in a source-only scan of a project
// whose exceptions come from its own modules or libraries.
class ExceptionLogging {
    private static final Logger log = LoggerFactory.getLogger(ExceptionLogging.class);

    void handle(AcmeException error, Request request) {
        log.debug("request failed", error);
        log.error("uri {} message {}", request.uri(), error.getMessage(), error);
        log.error("uri {}", request.uri(), request); // expect: java.log-format-arguments
        log.error("uri {}", request.uri(), request.id(), error); // expect: java.log-format-arguments
    }

    void local(Request request) {
        AcmeError failure = request.error();
        log.warn("request {} failed", request.id(), failure);
        Object other = request.error();
        log.warn("request {} failed", request.id(), other); // expect: java.log-format-arguments
    }
}
