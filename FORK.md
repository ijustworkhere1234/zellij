# About this fork

`ijustworkhere1234/zellij` is a personal fork of `zellij-org/zellij`. It exists so this machine can run Zellij built from source.

## Policy: track upstream `main` (the dev line)

- The fork follows **upstream `main`**, not a release tag. `main` reports the next unreleased version (e.g. 0.46.0 while the latest release is 0.45.1). This was a deliberate choice: on 2026-10-14 we tried v0.45.1 and switched back to dev.
- The working branch is `fix/crash-resilience`, which is upstream `main` plus our commits.
- The fork's `main` and `fix/crash-resilience` must both equal that local branch. The GitHub fork always reflects what is built and installed locally.

## Our commits on top of upstream

Our commits are **test-only**. Upstream `main` already fixes both crashes they cover:

1. `test(server): regression test for surviving accept() failures`: the IPC accept loop used to `panic!` on any accept() error (e.g. EMFILE), killing every session. Refs upstream #5474.
2. `test(server): regression test for removing a client with no session`: `RemoveClient` used to unwrap a `None` `session_data`, so the server panicked when a client disconnected before its session was initialized.
3. `chore: add libc dev-dependency for zellij-integration-tests`.

The tests live in `zellij-integration-tests/tests/server_resilience.rs`. They pass on current `main` and fail on v0.45.1, so they catch it if upstream ever regresses.

## Updating (pull latest upstream, rebuild, install)

```sh
git fetch upstream --tags
git rebase upstream/main                 # on fix/crash-resilience
git diff --stat upstream/main            # must show ONLY our test files + libc lines
git push --force-with-lease origin HEAD:main
git push --force-with-lease origin fix/crash-resilience
git rev-list --count origin/main..upstream/main   # must print 0
cargo x install /tmp/zellij-new && mv /tmp/zellij-new ~/.cargo/bin/zellij
git checkout -- zellij-utils/assets/plugins/      # the build rewrites these
cargo test -p zellij-integration-tests --test server_resilience
```

## Pitfalls we hit

- A rebase in Sep 2026 silently dropped the server-side code from our fix commits and left only the tests. After every rebase, check `git diff --stat upstream/main` and make sure you understand every file in it.
- Release builds rewrite the checked-in `zellij-utils/assets/plugins/*.wasm`. Discard those changes; don't commit them.
- Running sessions keep using the binary they started with. Restart them to pick up a new build.

## Backups (local only)

- Branch `backup/crash-resilience-on-v0.45.1`: v0.45.1 plus the full server-side fixes, in case we ever need stable.
- `~/.cargo/bin/zellij.0.45.1.bak`, `~/.cargo/bin/zellij.0.46-dev.bak`.
