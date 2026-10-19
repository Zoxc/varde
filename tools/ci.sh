#!/usr/bin/env bash
# The checks a change must pass, in order; stops at the first failure.
#
#   tools/ci.sh                 all steps
#   tools/ci.sh clippy test     just these steps
#   tools/ci.sh kernel-ignores extrude "curved boolean"
#                               that step alone, these capabilities landed too
#
# Steps: fmt, kernel-ignores, clippy, bans, test, wasm, deny, reuse,
# attribution.
# `kernel-ignores` counts the tests tagged #[ignore = "kernel: <capability>"]
# per capability and crate, and fails if one waits on a capability in
# KERNEL_LANDED (below) or given after it.
# `test` runs the workspace's tests, then the kernel crates' again on
# one rayon thread (their results must not depend on the thread count),
# then varde-decide's with hardware FMA on (x86_64; the same bits).
# `wasm` builds for the browser, then runs varde-decide's same-bits tests
# on wasm32-wasip1 under Node's WASI (tools/wasi_run.mjs): its results must
# be the bits the native run pins. It needs the wasm32-unknown-unknown and
# wasm32-wasip1 targets (`rustup target add`) and Node.js.
# `bans` checks clippy refuses each banned call in
# crates/kernel/tests/ban_probe.rs and that the kernel crates' clippy.toml
# copies agree. `deny` needs cargo-deny (`cargo install cargo-deny`) and
# `reuse` needs reuse (`pipx install reuse`); set CARGO_DENY or REUSE to
# use other binaries. Cargo's own variables apply, e.g. CARGO_BUILD_JOBS=4
# on a shared machine.

set -euo pipefail
cd "$(dirname "$0")/.."

KERNEL_CRATES=(-p varde-decide -p varde-kernel -p varde-testkit)
# The kernel capabilities that have landed (AGENTS.md): no test may still
# be ignored waiting on one. Add a capability when it lands, e.g.
# KERNEL_LANDED=(solids extrude "curved boolean").
KERNEL_LANDED=()
CARGO_DENY=${CARGO_DENY:-cargo-deny}
REUSE=${REUSE:-reuse}

step_fmt() {
    cargo fmt --all --check
}

step_clippy() {
    cargo clippy --workspace --all-targets --locked -- -D warnings
}

step_bans() {
    local f
    for f in crates/kernel/clippy.toml crates/testkit/clippy.toml; do
        cmp -s crates/decide/clippy.toml "$f" || {
            echo "$f differs from crates/decide/clippy.toml" >&2
            return 1
        }
    done
    local probe=crates/kernel/tests/ban_probe.rs
    local want out got
    want=$(grep -n '// banned$' "$probe" | cut -d: -f1 | sort -n | uniq)
    # The probe only builds with --cfg ban_probe. Clippy's lints are
    # warnings here, so a failing run is a build error: show it.
    out=$(cargo clippy --locked -p varde-kernel --test ban_probe \
            --message-format=short -- --cfg ban_probe 2>&1) || {
        echo "$out" >&2
        return 1
    }
    # A ban whose path doesn't resolve bans nothing, and clippy only says so
    # where the path's crate is loaded, as in this build.
    local unresolved
    unresolved=$(grep 'clippy\.toml:[0-9]*:[0-9]*: warning' <<< "$out") && {
        echo "$unresolved" >&2
        echo "bans: clippy.toml names paths clippy can't resolve" >&2
        return 1
    }
    got=$(sed -n "s|^$probe:\([0-9]*\):[0-9]*: warning: use of a disallowed.*|\1|p" <<< "$out" |
        sort -n | uniq)
    if [[ "$want" != "$got" ]]; then
        echo "clippy refused lines [$(echo $got)] of $probe, expected [$(echo $want)]" >&2
        return 1
    fi
    echo "bans: clippy refuses all $(echo "$want" | wc -l) banned calls"
}

step_kernel-ignores() {
    python3 -B tools/test_kernel_ignores.py
    # (The `+` form: bash before 4.4 calls an empty array unbound.)
    python3 -B tools/kernel_ignores.py ${KERNEL_LANDED[@]+"${KERNEL_LANDED[@]}"} "$@"
}

step_test() {
    cargo test --workspace --locked
    RAYON_NUM_THREADS=1 cargo test --locked "${KERNEL_CRATES[@]}"
    # The decision layer's results must be the same bits with hardware
    # FMA: its tests pin them (in their own target directory, as the flag
    # rebuilds everything).
    if [[ $(uname -m) == x86_64 ]]; then
        RUSTFLAGS="${RUSTFLAGS-} -C target-feature=+fma" \
            cargo test --locked -p varde-decide --target-dir target/fma
    fi
}

step_wasm() {
    cargo build --locked --target wasm32-unknown-unknown -p varde-decide -p varde-kernel
    cargo check --workspace --locked --target wasm32-unknown-unknown
    need node Node.js "install Node.js 22 or later"
    # Node 22 can collect garbage inside a WASI fast API call (fd_write's
    # allocation) and crash with SIGSEGV; the slow call path is safe.
    CARGO_TARGET_WASM32_WASIP1_RUNNER="node --no-warnings --no-turbo-fast-api-calls $PWD/tools/wasi_run.mjs" \
        cargo test --locked -p varde-decide --target wasm32-wasip1 -- same_bits_everywhere
}

need() {
    command -v "$1" > /dev/null || {
        echo "$2 not found: $3" >&2
        return 1
    }
}

step_deny() {
    need "$CARGO_DENY" cargo-deny "cargo install --locked cargo-deny, or set CARGO_DENY"
    "$CARGO_DENY" check bans licenses sources
    # Advisories change by the day and need the network: reported, not a
    # gate (deny.toml).
    "$CARGO_DENY" check advisories || echo "deny: advisories reported problems (not a gate)" >&2
}

# reuse lint, failing on every license finding (a file without a license,
# a bad, deprecated or unused license, one without its text in LICENSES/,
# unreadable files) but not on missing copyright: our own files carry
# none (REUSE.toml), so the lint's overall verdict would always fail.
step_reuse() {
    need "$REUSE" reuse "pipx install reuse, or set REUSE"
    local out
    out=$("$REUSE" lint --json) || [[ -n $out ]] || return 1
    python3 -c '
import json, sys
report = json.loads(sys.stdin.read())
bad = {k: v for k, v in report["non_compliant"].items()
       if v and k != "missing_copyright_info"}
for kind, items in sorted(bad.items()):
    for item in items if isinstance(items, list) else [items]:
        print(f"reuse: {kind}: {item}", file=sys.stderr)
s = report["summary"]
licensed, total = s["files_with_licensing_info"], s["files_total"]
print(f"reuse: {licensed} of {total} files licensed, {len(bad)} kind(s) of license problem")
sys.exit(1 if bad else 0)
' <<< "$out"
}

step_attribution() {
    python3 tools/test_check_attribution.py
    python3 tools/check_attribution.py
}

ALL=(fmt kernel-ignores clippy bans test wasm deny reuse attribution)
is_step() {
    local s
    for s in "${ALL[@]}"; do
        [[ $1 == "$s" ]] && return
    done
    return 1
}
# `kernel-ignores` followed by anything but steps: capabilities.
capabilities=0
if [[ ${1-} == kernel-ignores ]]; then
    for a in "${@:2}"; do
        is_step "$a" || capabilities=1
    done
fi
if ((capabilities)); then
    shift
    echo "== kernel-ignores"
    step_kernel-ignores "$@"
    echo "== all passed: kernel-ignores $*"
    exit
fi
steps=("$@")
[[ ${#steps[@]} -gt 0 ]] || steps=("${ALL[@]}")
for s in "${steps[@]}"; do
    is_step "$s" || {
        echo "unknown step $s; steps: ${ALL[*]}" >&2
        exit 2
    }
done
for s in "${steps[@]}"; do
    echo "== $s"
    "step_$s"
done
echo "== all passed: ${steps[*]}"
