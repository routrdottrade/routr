#!/usr/bin/env bash
set -euo pipefail
C="$(cd "$(dirname "$0")/../contracts" && pwd)"
WS=$(sed -n 's/^repository = "\([^"]*\)"/\1/p' "$C/Cargo.toml")
[ -n "$WS" ] || { echo "contracts/Cargo.toml has no workspace repository field"; exit 1; }
bad=0
for crate in token locker factory routr nft-factory shades nft-market; do
  f="$C/$crate/src/lib.rs"
  [ -f "$f" ] || continue  # a mirror carries only its own crates
  own=$(sed -n 's/^repository = "\([^"]*\)"/\1/p' "$C/$crate/Cargo.toml")
  REPO=${own:-$WS}
  n=$(grep -c 'link = "' "$f" || true)
  link=$(sed -n 's/.*link = "\([^"]*\)".*/\1/p' "$f" | head -1)
  if [ "$n" != "1" ] || [ "$link" != "$REPO" ]; then
    echo "MISMATCH $crate: link = '${link:-<none>}' (x$n), repository = '$REPO'"; bad=1
  else
    echo "ok $crate: $link"
  fi
done
[ "$bad" = 0 ] || { echo "run scripts/set-repo-link.sh (or fix the odd one by hand)"; exit 1; }
echo "every NEP-330 link equals its crate's repository"
