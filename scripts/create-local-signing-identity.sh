#!/usr/bin/env bash
# Create a self-signed code-signing identity "Popizdim Local Signing" in the
# login keychain.
#
# Why: ad-hoc signed builds (signingIdentity "-") get a designated requirement
# of `cdhash H"…"`, which changes on every build, so macOS forgets the
# Accessibility and Microphone grants after each reinstall. Signing every local
# build with the same certificate gives a stable requirement
# (`identifier "com.popizdim.desktop" and certificate root = H"…"`), and the
# grants survive rebuilds.
#
# What it does: generates an RSA key and a 10-year self-signed certificate
# limited to code signing, imports both into the login keychain with access for
# /usr/bin/codesign, and deletes the key files. No admin password, no trust
# settings are changed.
#
# Then build with:
#   APPLE_SIGNING_IDENTITY="<SHA-1 printed below>" bun run tauri build ...
set -euo pipefail

NAME="Popizdim Local Signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
OPENSSL=/usr/bin/openssl # LibreSSL: writes PKCS#12 that `security` can read

work=$(mktemp -d)
chmod 700 "$work"
trap 'rm -rf "$work"' EXIT

cat >"$work/cert.cnf" <<EOF
[req]
distinguished_name = dn
prompt = no
x509_extensions = ext
[dn]
CN = $NAME
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF

"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -config "$work/cert.cnf" -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
pass=$("$OPENSSL" rand -hex 16)
"$OPENSSL" pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
  -name "$NAME" -passout "pass:$pass" -out "$work/id.p12"

security import "$work/id.p12" -k "$KEYCHAIN" -P "$pass" -T /usr/bin/codesign

echo
echo "Imported \"$NAME\". SHA-1 for APPLE_SIGNING_IDENTITY:"
"$OPENSSL" x509 -in "$work/cert.pem" -noout -fingerprint -sha1 |
  sed 's/.*=//; s/://g'
