#!/usr/bin/env bash
# Load every bench service's mock and demo data. Idempotent: run it after any
# `up`; a service that is not running is skipped, one that fails is reported
# and the rest still run.
#
#   ./seed/seed-all.sh            # everything that is up
#   ./seed/seed-all.sh redis s3   # just these scripts (seed-<name>.sh)
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
order=(postgres mysql mongo redis couch s3 qdrant webdav rabbitmq kafka nats mqtt
  forgejo jenkins teamcity redmine puppetdb k8s keycloak heavy)
[ $# -gt 0 ] && order=("$@")

failed=()
for name in "${order[@]}"; do
  script="$here/seed-$name.sh"
  if [ ! -f "$script" ]; then
    echo "no such seed script: $script" >&2
    failed+=("$name")
    continue
  fi
  bash "$script" || failed+=("$name")
done

echo
if [ ${#failed[@]} -gt 0 ]; then
  echo "Seeding finished with failures: ${failed[*]}" >&2
  exit 1
fi
echo "Seeding finished. Generated tokens are in $(cd "$here/.." && pwd)/.state/"
