#!/bin/sh
# Run a command and retry it after a failure. CI uses this for downloads, which fail
# now and then with HTTP 429 or a dropped connection.
# Usage: scripts/retry.sh COMMAND [ARGS...]
# RETRY_ATTEMPTS (default 3) and RETRY_DELAY (default 20 seconds, multiplied by the
# attempt number) tune it.
set -u

attempts=${RETRY_ATTEMPTS:-3}
delay=${RETRY_DELAY:-20}
attempt=1
while :; do
    "$@" && exit 0
    status=$?
    if [ "$attempt" -ge "$attempts" ]; then
        exit "$status"
    fi
    wait=$((delay * attempt))
    echo "retry.sh: attempt $attempt of $attempts failed with status $status; retrying in ${wait}s" >&2
    sleep "$wait"
    attempt=$((attempt + 1))
done
