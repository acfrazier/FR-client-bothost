#!/usr/bin/env bash

set -euo pipefail

ENGINE_DIR="${1:-$HOME/experiments/Server/engine}"
PRIVATE_KEY="$ENGINE_DIR/data/config/private.pem"

if [ ! -f "$PRIVATE_KEY" ]; then
    echo "ERROR: RSA private key not found:" >&2
    echo "  $PRIVATE_KEY" >&2
    exit 1
fi

command -v openssl >/dev/null 2>&1 || {
    echo "ERROR: openssl is required." >&2
    exit 1
}

command -v python3 >/dev/null 2>&1 || {
    echo "ERROR: python3 is required." >&2
    exit 1
}

echo "→ Extracting RSA key from:"
echo "  $PRIVATE_KEY"

# Extract modulus.
MODULUS_HEX="$(
    openssl rsa \
        -in "$PRIVATE_KEY" \
        -noout \
        -modulus |
    sed 's/^Modulus=//'
)"

if [ -z "$MODULUS_HEX" ]; then
    echo "ERROR: Could not extract RSA modulus." >&2
    exit 1
fi

LOGIN_RSAN="$(
    python3 -c "print(int('$MODULUS_HEX',16))"
)"

# Extract public exponent.
# OpenSSL 3.x prints:  publicExponent: 65537 (0x10001)         (single line)
# Older / LibreSSL:    publicExponent:\n    <hex bytes>        (multi line)
LOGIN_RSAE="$(
    openssl rsa -in "$PRIVATE_KEY" -noout -text \
    | sed -n 's/^publicExponent: \([0-9][0-9]*\).*/\1/p'
)"

if [ -z "$LOGIN_RSAE" ]; then
    # Fallback: multi-line hex format (older OpenSSL / LibreSSL)
    E_HEX="$(
        openssl rsa -in "$PRIVATE_KEY" -noout -text |
        awk '
            /^publicExponent:/ { found=1; next }
            found {
                line=$0; gsub(/[ :]/, "", line)
                if ($0 ~ /^[[:space:]]*[a-zA-Z][a-zA-Z ]*:/) exit
                if (line ~ /^[0-9A-Fa-f]+$/) printf "%s", line
            }
            END { print "" }
        '
    )"
    if [ -n "$E_HEX" ]; then
        LOGIN_RSAE="$(python3 -c "print(int('$E_HEX',16))")"
    fi
fi

if [ -z "$LOGIN_RSAE" ]; then
    echo "ERROR: Could not extract RSA public exponent." >&2
    echo "Relevant OpenSSL output:" >&2

    openssl rsa \
        -in "$PRIVATE_KEY" \
        -noout \
        -text 2>&1 |
    sed -n '/publicExponent:/,/privateExponent:/p' >&2

    exit 1
fi

echo "→ RSA key extracted successfully."
echo "→ RSA exponent: $LOGIN_RSAE"
echo "→ RSA modulus:  $LOGIN_RSAN..."

echo "→ Building client-play with baked RSA key..."

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

LOGIN_RSAE="$LOGIN_RSAE" \
LOGIN_RSAN="$LOGIN_RSAN" \
cargo build -p client-play
