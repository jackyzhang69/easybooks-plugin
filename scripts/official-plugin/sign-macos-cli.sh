#!/usr/bin/env bash
# Isolated Developer ID sign + notarytool for official plugin CLIs.
# Shared by EasyBooks, AnyChat, AnyDoc (FormBro keeps worker entitlements in its
# own job but can call `sign` / `notarize` with --entitlements).
#
# Usage:
#   sign-macos-cli.sh bootstrap
#   sign-macos-cli.sh sign <mach-o> [--entitlements FILE]
#   sign-macos-cli.sh notarize <mach-o-or-dir>
#   sign-macos-cli.sh teardown
#
# Required env for bootstrap: APPLE_SIGNING_IDENTITY, CSC_LINK, CSC_KEY_PASSWORD
# Required env for notarize:  APPLE_ID, APPLE_TEAM_ID, and
#   APPLE_APP_SPECIFIC_PASSWORD or APPLE_NOTARIZE_PASS
set -euo pipefail

STATE="${SIGN_STATE_FILE:-${RUNNER_TEMP:-/tmp}/macos-cli-signing.state}"

emit() {
  if [ -n "${GITHUB_ENV:-}" ]; then
    printf '%s\n' "$1" >> "$GITHUB_ENV"
  fi
}

load_state() {
  [ -f "$STATE" ] || { echo "sign-macos-cli: missing state $STATE (run bootstrap first)" >&2; exit 1; }
  # shellcheck disable=SC1090
  set -a
  # shellcheck disable=SC1091
  . "$STATE"
  set +a
}

cmd="${1:-}"
shift || true

case "$cmd" in
  bootstrap)
    : "${APPLE_SIGNING_IDENTITY:?}"
    : "${CSC_LINK:?}"
    : "${CSC_KEY_PASSWORD:?}"
    PREFIX="${SIGN_KEYCHAIN_PREFIX:-plugin-cli-signing}"
    KEYCHAIN="${RUNNER_TEMP:-/tmp}/${PREFIX}.keychain-db"
    P12="${RUNNER_TEMP:-/tmp}/${PREFIX}.p12"
    ORIG_DEFAULT="${RUNNER_TEMP:-/tmp}/${PREFIX}.orig-default"
    ORIG_SEARCH="${RUNNER_TEMP:-/tmp}/${PREFIX}.orig-search"
    KC_PW="$(openssl rand -base64 24)"
    security default-keychain -d user | sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//' > "$ORIG_DEFAULT" || true
    security list-keychains -d user | sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//' > "$ORIG_SEARCH" || true
    printf '%s' "$CSC_LINK" | openssl base64 -d -A > "$P12"
    security create-keychain -p "$KC_PW" "$KEYCHAIN"
    security set-keychain-settings -ut 21600 "$KEYCHAIN"
    security unlock-keychain -p "$KC_PW" "$KEYCHAIN"
    security import "$P12" -k "$KEYCHAIN" -P "$CSC_KEY_PASSWORD" \
      -T /usr/bin/codesign -T /usr/bin/security -T /usr/bin/productsign
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KC_PW" "$KEYCHAIN" >/dev/null
    rm -f "$P12"
    for ca in DeveloperIDG2CA DeveloperIDCA; do
      curl -fsSL "https://www.apple.com/certificateauthority/${ca}.cer" -o "${RUNNER_TEMP:-/tmp}/${ca}.cer"
      security import "${RUNNER_TEMP:-/tmp}/${ca}.cer" -k "$KEYCHAIN" >/dev/null 2>&1 || true
      rm -f "${RUNNER_TEMP:-/tmp}/${ca}.cer"
    done
    existing=()
    while IFS= read -r kc; do [ -n "$kc" ] && existing+=("$kc"); done < "$ORIG_SEARCH"
    if [ "${#existing[@]}" -gt 0 ]; then
      security list-keychains -d user -s "$KEYCHAIN" "${existing[@]}"
    else
      security list-keychains -d user -s "$KEYCHAIN"
    fi
    IDENTITY_HASH="$(security find-identity -v -p codesigning "$KEYCHAIN" | grep -F "$APPLE_SIGNING_IDENTITY" | grep -oE '[0-9A-Fa-f]{40}' | head -1)"
    if ! printf '%s' "$IDENTITY_HASH" | grep -qE '^[0-9A-Fa-f]{40}$'; then
      echo "sign-macos-cli: no unambiguous Developer ID identity in the isolated keychain" >&2
      exit 1
    fi
    PROBE="${RUNNER_TEMP:-/tmp}/${PREFIX}-probe"
    cp /bin/echo "$PROBE"
    codesign --force --options runtime --keychain "$KEYCHAIN" --sign "$IDENTITY_HASH" "$PROBE"
    codesign --verify --strict "$PROBE"
    rm -f "$PROBE"
    umask 077
    cat > "$STATE" <<EOF
SIGN_KEYCHAIN=$KEYCHAIN
SIGN_IDENTITY_HASH=$IDENTITY_HASH
SIGN_ORIG_DEFAULT=$ORIG_DEFAULT
SIGN_ORIG_SEARCH=$ORIG_SEARCH
EOF
    emit "SIGN_KEYCHAIN=$KEYCHAIN"
    emit "SIGN_IDENTITY_HASH=$IDENTITY_HASH"
    emit "SIGN_ORIG_DEFAULT=$ORIG_DEFAULT"
    emit "SIGN_ORIG_SEARCH=$ORIG_SEARCH"
    emit "CODESIGN_IDENTITY=$IDENTITY_HASH"
    echo "sign-macos-cli: bootstrap ok"
    ;;

  sign)
    load_state
    BIN="${1:?mach-o path}"
    shift || true
    ENTITLEMENTS=()
    if [ "${1:-}" = "--entitlements" ]; then
      ENTITLEMENTS=(--entitlements "${2:?}")
    fi
    test -f "$BIN"
    chmod +x "$BIN" || true
    codesign --force --options runtime --timestamp --keychain "$SIGN_KEYCHAIN" \
      --sign "$SIGN_IDENTITY_HASH" "${ENTITLEMENTS[@]}" "$BIN"
    codesign --verify --strict --verbose=2 "$BIN"
    echo "sign-macos-cli: signed $BIN"
    ;;

  notarize)
    load_state
    TARGET="${1:?mach-o or directory}"
    PASS="${APPLE_APP_SPECIFIC_PASSWORD:-${APPLE_NOTARIZE_PASS:-}}"
    : "${APPLE_ID:?}"
    : "${APPLE_TEAM_ID:?}"
    : "${PASS:?APPLE_APP_SPECIFIC_PASSWORD or APPLE_NOTARIZE_PASS required}"
    ZIP="${RUNNER_TEMP:-/tmp}/plugin-cli-notarize-$$.zip"
    rm -f "$ZIP"
    if [ -d "$TARGET" ]; then
      ditto -c -k --keepParent "$TARGET" "$ZIP"
    else
      ditto -c -k --keepParent "$TARGET" "$ZIP"
    fi
    xcrun notarytool submit "$ZIP" \
      --apple-id "$APPLE_ID" \
      --team-id "$APPLE_TEAM_ID" \
      --password "$PASS" \
      --wait
    rm -f "$ZIP"
    echo "sign-macos-cli: notarized $TARGET"
    ;;

  teardown)
    if [ -f "$STATE" ]; then
      # shellcheck disable=SC1090
      set -a
      # shellcheck disable=SC1091
      . "$STATE"
      set +a
    fi
    KEYCHAIN_PATH="${SIGN_KEYCHAIN:-}"
    ORIG_DEFAULT_PATH="${SIGN_ORIG_DEFAULT:-}"
    ORIG_SEARCH_PATH="${SIGN_ORIG_SEARCH:-}"
    if [ -n "$KEYCHAIN_PATH" ]; then
      security delete-keychain "$KEYCHAIN_PATH" 2>/dev/null || true
    fi
    if [ -n "$ORIG_DEFAULT_PATH" ] && [ -s "$ORIG_DEFAULT_PATH" ]; then
      security default-keychain -s "$(cat "$ORIG_DEFAULT_PATH")" 2>/dev/null || true
    fi
    if [ -n "$ORIG_SEARCH_PATH" ] && [ -s "$ORIG_SEARCH_PATH" ]; then
      orig=()
      while IFS= read -r kc; do [ -n "$kc" ] && orig+=("$kc"); done < "$ORIG_SEARCH_PATH"
      [ "${#orig[@]}" -gt 0 ] && security list-keychains -d user -s "${orig[@]}" 2>/dev/null || true
    fi
    rm -f "$STATE" "$ORIG_DEFAULT_PATH" "$ORIG_SEARCH_PATH" 2>/dev/null || true
    echo "sign-macos-cli: teardown ok"
    ;;

  *)
    echo "usage: sign-macos-cli.sh bootstrap|sign|notarize|teardown" >&2
    exit 2
    ;;
esac
