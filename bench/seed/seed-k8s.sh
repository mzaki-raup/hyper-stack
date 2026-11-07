#!/usr/bin/env bash
# k3s (profile k8s): apply namespaces `shop` and `ops` (seed/data/k8s/*.yaml)
# and write two kubeconfigs, with the cluster, user and context named hs-bench:
#   .state/k3s/kubeconfig.yaml        server https://$BENCH_HOST:6443 (other machines)
#   .state/k3s/kubeconfig-local.yaml  server https://127.0.0.1:6443 (this box)
# `kubectl apply` makes a re-run a no-op.
SEED_NAME=k8s
source "$(dirname "$0")/lib.sh"
running k3s || { log "not running - skipped"; exit 0; }
wait_healthy k3s 60

k() { dc exec -T k3s kubectl "$@"; }

# /readyz answers before the node registers; wait for the node itself.
for _ in $(seq 1 30); do
  k get nodes -o name 2>/dev/null | grep -q . && break
  sleep 5
done
k wait --for=condition=Ready node --all --timeout=180s >/dev/null

# A pod applied right after its namespace can be refused until the namespace's
# default service account exists, so retry the whole apply a few times.
for f in "$DATA_DIR"/k8s/*.yaml; do
  for attempt in 1 2 3 4 5; do
    if out=$(k apply -f - < "$f" 2>&1); then
      break
    fi
    [ "$attempt" = 5 ] && { printf '%s\n' "$out" >&2; exit 1; }
    sleep 5
  done
  printf '%s\n' "$out" | grep -v ' unchanged$' | sed 's/^/  /' || true
done

# Kubeconfigs, renamed from k3s' "default" so the context reads as the bench.
mkdir -p "$STATE_DIR/k3s"
raw=$(dc exec -T k3s cat /etc/rancher/k3s/k3s.yaml | tr -d '\r' |
  sed -e 's/^\([ -]*\)\(name\|cluster\|user\|current-context\): default$/\1\2: hs-bench/')
if printf '%s\n' "$raw" | grep -qE ': default$'; then
  warn "k3s.yaml has a 'default' name this seed did not rename"
  exit 1
fi
printf '%s\n' "$raw" | sed "s#server: https://127.0.0.1:6443#server: https://$BENCH_HOST:6443#" \
  > "$STATE_DIR/k3s/kubeconfig.yaml"
printf '%s\n' "$raw" > "$STATE_DIR/k3s/kubeconfig-local.yaml"
chmod 600 "$STATE_DIR/k3s/kubeconfig.yaml" "$STATE_DIR/k3s/kubeconfig-local.yaml"

# Give the healthy workloads a moment, then show the picture.
k -n shop rollout status deploy/shop-web --timeout=180s >/dev/null || warn "shop-web is not ready yet"
k -n shop rollout status statefulset/cache --timeout=180s >/dev/null || warn "cache is not ready yet"
k -n shop wait --for=condition=complete job/db-migrate --timeout=180s >/dev/null || warn "db-migrate has not completed yet"
log "pods:"
k get pods -A --no-headers 2>/dev/null | grep -E '^(shop|ops) ' | awk '{ printf "  %-5s %-28s %-6s %s\n", $1, $2, $3, $4 }'
log "kubeconfig: .state/k3s/kubeconfig.yaml (https://$BENCH_HOST:6443), .state/k3s/kubeconfig-local.yaml (https://127.0.0.1:6443), context hs-bench"
