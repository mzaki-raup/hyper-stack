#!/usr/bin/env bash
# Apache Hop: copies Hop's own sample projects (pipelines .hpl, workflows .hwf,
# metadata) out of the image into .state/hop-samples, the files the ETL
# activity's phase 1 reads (docs/tools/etl.md §8.4; they carry the ASF licence
# headers, so vendor them with their provenance if they are committed), and
# runs one sample pipeline to show the runtime works. Re-running copies again.
SEED_NAME=hop
source "$(dirname "$0")/lib.sh"
running hop || { log "not running - skipped"; exit 0; }

dest="$STATE_DIR/hop-samples"
rm -rf "$dest"
mkdir -p "$dest"
id=$(dc ps -q hop)
src=""
for cand in /opt/hop/config/projects/samples /usr/local/tomcat/webapps/ROOT/config/projects/samples; do
  docker exec "$id" test -d "$cand" 2>/dev/null && { src=$cand; break; }
done
if [ -z "$src" ]; then
  src=$(docker exec "$id" sh -c 'find / -type d -path "*/projects/samples" 2>/dev/null | head -1')
fi
[ -n "$src" ] || { warn "no samples project found in the image"; exit 1; }
docker cp "$id:$src/." "$dest/" >/dev/null
hpl=$(find "$dest" -name '*.hpl' | wc -l)
hwf=$(find "$dest" -name '*.hwf' | wc -l)
log "copied $src to .state/hop-samples: $hpl pipelines, $hwf workflows"

# Run one: Hop's own "hello world" style pipeline, with the samples project.
sample=$(docker exec "$id" sh -c "find $src -name '*.hpl' -path '*transforms*' | head -1")
if [ -n "$sample" ]; then
  if docker exec "$id" sh -c "cd /opt/hop && ./hop-run.sh --file='$sample' --runconfig=local --project=samples --level=Basic" >"$STATE_DIR/hop-run.log" 2>&1; then
    log "ran $(basename "$sample"): ok"
  else
    warn "hop-run did not finish on $(basename "$sample") - the samples are copied all the same: $(tail -3 "$STATE_DIR/hop-run.log" | tr '\n' ' ')"
  fi
fi
