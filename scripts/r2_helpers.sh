#!/usr/bin/env bash

# Shared fail-closed helpers for GitHub workflows that read optional R2 state.
# A missing object is expected during first publication; authentication,
# transport, rate-limit, and server failures must abort instead of looking empty.

R2_OPTIONAL_MISSING=44

r2_get_optional() {
  if [ "$#" -ne 2 ]; then
    echo "usage: r2_get_optional <s3-uri> <destination>" >&2
    return 2
  fi
  local uri=$1
  local destination=$2
  local error_file
  local status
  error_file=$(mktemp)

  if aws s3 cp "$uri" "$destination" \
      --endpoint-url "$AWS_ENDPOINT_URL" \
      --only-show-errors 2>"$error_file"; then
    rm -f "$error_file"
    return 0
  else
    status=$?
  fi

  if grep -Eiq '(\(404\)|NoSuchKey|Not[[:space:]]+Found)' "$error_file"; then
    echo "Optional R2 object is absent: $uri" >&2
    rm -f "$error_file"
    return "$R2_OPTIONAL_MISSING"
  fi

  echo "Failed to read R2 object: $uri" >&2
  cat "$error_file" >&2
  rm -f "$error_file"
  return "$status"
}
