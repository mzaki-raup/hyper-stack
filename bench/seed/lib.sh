#!/usr/bin/env bash
# Shared helpers for the seed scripts. Sourced, not run.
# Needs only docker (with the compose plugin) and curl on the host.

set -euo pipefail

BENCH_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SEED_DIR="$BENCH_DIR/seed"
DATA_DIR="$SEED_DIR/data"
STATE_DIR="$BENCH_DIR/.state"
mkdir -p "$STATE_DIR"
cd "$BENCH_DIR"

# Read .env without evaluating it: KEY=value lines, optional surrounding quotes.
load_env() {
  local file="$BENCH_DIR/.env" line key val
  if [ ! -f "$file" ]; then
    echo "No $file - run: cp .env.example .env" >&2
    exit 1
  fi
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%$'\r'}
    case "$line" in '' | '#'*) continue ;; esac
    key=${line%%=*}
    val=${line#*=}
    case "$val" in
      \'*\') val=${val:1:${#val}-2} ;;
      \"*\") val=${val:1:${#val}-2} ;;
    esac
    [ -n "${!key+x}" ] || export "$key=$val"   # the caller's environment wins
  done < "$file"
}
load_env

H=${SEED_HOST:-127.0.0.1}   # where the published ports are reached from this shell

dc() { docker compose "$@"; }
log() { printf '\033[1m[%s]\033[0m %s\n' "${SEED_NAME:-seed}" "$*"; }
warn() { printf '\033[33m[%s] %s\033[0m\n' "${SEED_NAME:-seed}" "$*" >&2; }

# Is a compose service running?
running() {
  [ -n "$(dc ps --status running -q "$1" 2>/dev/null)" ]
}

# Wait until a service's healthcheck says healthy (or it has none and is running).
wait_healthy() {
  local svc=$1 tries=${2:-90} id state i
  for ((i = 0; i < tries; i++)); do
    id=$(dc ps -q "$svc" 2>/dev/null || true)
    if [ -n "$id" ]; then
      state=$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$id" 2>/dev/null || true)
      case "$state" in healthy | running) return 0 ;; esac
    fi
    sleep 5
  done
  warn "$svc is not healthy after $((tries * 5))s"
  return 1
}

# Wait for an HTTP endpoint to answer with any of the given codes (default 2xx).
wait_http() {
  local url=$1 tries=${2:-60} code i
  for ((i = 0; i < tries; i++)); do
    code=$(curl -s -o /dev/null -w '%{http_code}' "$url" || true)
    case "$code" in 2??) return 0 ;; esac
    sleep 5
  done
  warn "$url did not answer after $((tries * 5))s"
  return 1
}

# Save a generated value (a token) so later runs and the README's readers find it.
save_state() { printf '%s\n' "$2" > "$STATE_DIR/$1"; }
