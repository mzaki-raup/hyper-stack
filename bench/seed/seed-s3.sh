#!/usr/bin/env bash
# S3 (MinIO): buckets `hyperstack` and `backups`, and the nested objects the
# S3 activity's folder tree was tested with. Signed with curl's --aws-sigv4,
# so this also proves the access key and secret work.
SEED_NAME=s3
source "$(dirname "$0")/lib.sh"
running s3 || { log "not running - skipped"; exit 0; }
wait_healthy s3

S="http://$H:${S3_PORT:-9000}"
s3() {
  curl -s --aws-sigv4 "aws:amz:$S3_REGION:s3" --user "$S3_ACCESS_KEY:$S3_SECRET_KEY" \
    -H 'x-amz-content-sha256: UNSIGNED-PAYLOAD' "$@"
}

for b in hyperstack backups; do
  code=$(s3 -o /dev/null -w '%{http_code}' -X PUT "$S/$b")
  case "$code" in 200 | 409) ;; *) warn "create bucket $b: HTTP $code"; exit 1 ;; esac
done

root="$DATA_DIR/s3"
count=0
while IFS= read -r -d '' f; do
  key=${f#"$root"/}
  s3 -f -o /dev/null -T "$f" "$S/$key"
  count=$((count + 1))
done < <(find "$root" -type f -print0)
log "uploaded $count objects; hyperstack holds: $(s3 "$S/hyperstack?list-type=2" | grep -o '<Key>[^<]*' | sed 's/<Key>//' | tr '\n' ' ')"
