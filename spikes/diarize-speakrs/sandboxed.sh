#!/bin/sh
# Run a command inside sandbox.sb with a scrubbed environment.
SPIKE="$(cd "$(dirname "$0")" && pwd)"
exec /usr/bin/sandbox-exec -D SPIKE="$SPIKE" -f "$SPIKE/sandbox.sb" \
  /usr/bin/env -i HOME=/Users/asmadey PATH="/Users/asmadey/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  TMPDIR="$SPIKE/target/tmp" CARGO_TARGET_DIR="$SPIKE/target" "$@"
