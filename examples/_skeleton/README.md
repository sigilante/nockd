# _skeleton — known-good starting point for a nockd example

Copy this directory to `examples/<your-app>/`, then follow the recipe. It bundles the
toolchain workarounds from [`../GOTCHAS.md`](../GOTCHAS.md) so you start from green.

## Files here
- `rust-toolchain.toml` — pins the Rust toolchain nockup's rev needs (avoids the `cold_path
  E0658` error). **Keep this in your project root.**
- `nockd.toml` — the one-command deploy manifest. Edit the `app`, the args, the metric
  recipe, and the endpoint.
- `cargo-tls-pin.toml` — paste into your `Cargo.toml` **only if** the app opens `https://`
  (chain readers); prevents the rustls dual-provider panic. Then add the `install_default()`
  line to `main()` (see that file).

## Recipe

```sh
# 1. scaffold from a nockup template (basic | http-server | grpc | chain | …)
#    Note (upstream): init nests under <name>/ and leaves rev="" — flatten + set rev=6d29078.
nockup project init

# 2. apply the skeleton workarounds
cp /path/to/_skeleton/rust-toolchain.toml .
#    (for TLS apps) paste cargo-tls-pin.toml into Cargo.toml + add install_default() to main

# 3. write minimal Hoon in hoon/app/app.hoon — change the template as little as possible.
#    If your app has a key number, LOG IT ON ONE LINE: e.g.  metric: requests=42

# 4. build (fix Hoon errors and repeat — Hoon is unforgiving)
nockup project build

# 5. deploy + verify
cp /path/to/_skeleton/nockd.toml .   # then edit it
nockd deploy -f nockd.toml
nockd ps        # running · verified · (healthy)
nockd logs <name>
```

## Definition of done
See [`../GOTCHAS.md`](../GOTCHAS.md#definition-of-done-per-example). In short: builds clean,
deploys, shows verified in `ps`, produces observable output, has a README. Commit when green.
