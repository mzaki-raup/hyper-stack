#!/usr/bin/env bash
# Recreate HyperStack's two git fixtures from scratch (the .git trees are not
# copied into the repository):
#
#   hs-git-fixture/   main / feature / topic / fix branches, a merge of `fix`
#                     paused on a README.md conflict, an unstaged change to
#                     main.rs, a stash (with an untracked file) and a tag.
#   hs-bare.git       a bare clone of shop-service (the "Localhost" source).
#   shop-service/     main / develop / feature/payments / topic / fix, tags
#                     v0.1.0 (annotated) and v0.2.0, a stash, an unstaged
#                     change, an untracked Cargo.lock, and a merge of `fix`
#                     paused on a README.md conflict.
#
# Usage: make-git-fixtures.sh [target-dir]   (default: ./git-fixtures)
# Re-running deletes and rebuilds only the three folders above inside
# target-dir. Commit dates are fixed so the history looks the same every time.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
target=${1:-./git-fixtures}
mkdir -p "$target"
target=$(cd "$target" && pwd)

# Throwaway fixture repositories: a fixed identity, no signing, no hooks.
g() {
  git -c user.name="Bench Dev" -c user.email="dev@example.test" \
      -c commit.gpgsign=false -c tag.gpgsign=false -c core.hooksPath=/dev/null \
      -c init.defaultBranch=main -c advice.detachedHead=false "$@"
}
tick=0
commit() { # commit <message>
  tick=$((tick + 1))
  local when="2026-10-$(printf '%02d' $((10 + tick / 4))) $(printf '%02d' $((9 + tick % 4))):00:00 +0000"
  GIT_AUTHOR_DATE="$when" GIT_COMMITTER_DATE="$when" g commit -q -m "$1"
}

# ---------------------------------------------------------------- hs-git-fixture
fx="$target/hs-git-fixture"
rm -rf "$fx"
g init -q "$fx"
cd "$fx"
cat > Cargo.toml <<'EOF'
[package]
name = "fixture"
version = "0.1.0"
edition = "2024"
EOF
echo '# Fixture' > README.md
cat > main.rs <<'EOF'
fn main() {
    println!("hello");
}
EOF
g add -A && commit "initial"
echo '# Fixture repo' > README.md
cat > main.rs <<'EOF'
fn main() {
    println!("hello, world");
    println!("second line");
}
EOF
g add -A && commit "test commit from HyperStack"
g tag -a v0.1.0 -m "First fixture release"
g switch -q -c topic
echo 'topic work' > topic.txt
g add -A && commit "topic: add topic.txt"
g switch -q main
GIT_AUTHOR_DATE="2026-10-12 12:00:00 +0000" GIT_COMMITTER_DATE="2026-10-12 12:00:00 +0000" \
  g merge -q --no-ff topic -m "merge topic"
g switch -q -c feature
sed -i 's/second line"/second line (feature)"/' main.rs
g add -A && commit "feature: tweak second line"
g switch -q main
sed -i 's/second line"/second line (main)"/' main.rs
g add -A && commit "main: tweak second line"
g switch -q -c fix
echo '# Fixture (fix branch)' > README.md
g add -A && commit "fix: readme on fix branch"
g switch -q main
# A stash holding a tracked change and an untracked file.
echo '    // scratch' >> main.rs
echo 'notes' > TODO.txt
g stash push -q -u -m "WIP: scratch notes"
echo '# Fixture (main)' > README.md
g add -A && commit "docs: readme on main"
# The unstaged change, then the merge that stops on README.md.
sed -i 's/hello, world/hello, fixture/' main.rs
g merge --no-ff fix -m "Merge branch 'fix'" >/dev/null 2>&1 || true
test -f .git/MERGE_HEAD || { echo "hs-git-fixture: expected a paused merge" >&2; exit 1; }

# ---------------------------------------------------------------- shop-service
bare="$target/hs-bare.git"
shop="$target/shop-service"
rm -rf "$bare" "$shop"
g init -q "$shop"
cd "$shop"
mkdir -p src docs
echo 'target/' > .gitignore
cat > Cargo.toml <<'EOF'
[package]
name = "shop-service"
version = "0.3.0"
edition = "2021"
EOF
cat > README.md <<'EOF'
# Shop Service

Orders, carts and payments for the demo shop.

## Running

    cargo run -- --port 8080
EOF
cp "$here/git/shop-service-architecture.md" docs/architecture.md
cat > src/cart.rs <<'EOF'
pub struct Cart {
    pub items: Vec<(String, u32)>,
}

impl Cart {
    pub fn new() -> Self {
        Cart { items: Vec::new() }
    }
}
EOF
cat > src/main.rs <<'EOF'
mod cart;

fn main() {
    let port = std::env::args().nth(2).unwrap_or_else(|| "8080".into());
    println!("shop-service listening on {port}");
}
EOF
g add -A && commit "feat: first cut of the order service"
cat > src/cart.rs <<'EOF'
pub struct Cart {
    pub items: Vec<(String, u32)>,
}

impl Cart {
    pub fn new() -> Self {
        Cart { items: Vec::new() }
    }

    pub fn add(&mut self, sku: &str, qty: u32) {
        self.items.push((sku.into(), qty));
    }
}
EOF
g add -A && commit "feat(cart): add items to a cart"
g tag -a v0.1.0 -m "First release"
printf '\n## Configuration\n\nSet `SHOP_DB_URL` to the database.\n' >> README.md
g add -A && commit "docs: describe the configuration"
base=$(g rev-parse HEAD)

g switch -q -c feature/payments
cat > src/payments.rs <<'EOF'
pub fn charge(amount_cents: u64) -> Result<String, String> {
    if amount_cents == 0 { return Err("nothing to charge".into()); }
    Ok(format!("ch_{amount_cents}"))
}
EOF
g add -A && commit "feat(payments): charge a card"
g switch -q -c topic "$base"
echo '// TODO: totals' >> src/cart.rs
g add -A && commit "chore: note the missing totals"
g switch -q -c fix "$base"
sed -i 's/for the demo shop\./for the demo shop (EU region)./' README.md
g add -A && commit "fix(docs): name the region"
g switch -q main
sed -i 's/for the demo shop\./for the demo shop, served over HTTP./' README.md
g add -A && commit "docs: say how the service is served"
sed -i 's|^    println!("shop-service listening on {port}");|&\n    let _cart = cart::Cart::new();|' src/main.rs
g add -A && commit "refactor: create the cart at start"
echo '- 2026-10-12 cart created at start' > CHANGES.md
g add -A && commit "docs: start a change log"
g tag v0.2.0
g branch develop

# The bare "origin", with every branch and tag pushed.
g clone -q --bare "$shop" "$bare"
g remote add origin ../hs-bare.git
g fetch -q origin
for b in main develop feature/payments topic fix; do g branch -q --set-upstream-to="origin/$b" "$b"; done

# Stash (untracked notes.txt), then the working-tree state the screenshots show.
echo 'scratch' > notes.txt
g stash push -q -u -m "WIP: scratch notes"
cat > src/main.rs <<'EOF'
mod cart;

fn main() {
    let port = std::env::args().nth(2).unwrap_or_else(|| "8080".into());
    println!("shop-service listening on {port}");
    let mut cart = cart::Cart::new();
    cart.add("sku-1001", 2);
    println!("{} item(s) in the demo cart", cart.items.len());
}
EOF
cat > Cargo.lock <<'EOF'
# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 4

[[package]]
name = "shop-service"
version = "0.3.0"
EOF
g merge --no-ff fix -m "Merge branch 'fix'" >/dev/null 2>&1 || true
test -f .git/MERGE_HEAD || { echo "shop-service: expected a paused merge" >&2; exit 1; }

echo "Git fixtures written to $target:"
for r in hs-git-fixture shop-service; do
  echo "  $r: $(g -C "$target/$r" branch --format='%(refname:short)' | tr '\n' ' ')| tags: $(g -C "$target/$r" tag | tr '\n' ' ')| stashes: $(g -C "$target/$r" stash list | wc -l) | merge paused on: $(g -C "$target/$r" diff --name-only --diff-filter=U | tr '\n' ' ')"
done
echo "  hs-bare.git: bare clone of shop-service"
