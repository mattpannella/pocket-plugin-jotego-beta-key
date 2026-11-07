# Jotego Beta Key plugin

A [Pocket plugin](https://github.com/openfpga-library/pocket-plugin) for pupdate and Pocket Sync.
It is the jtbeta auto-fetch that used to live inside pupdate (the `jt_beta_github_fetch`,
`jt_beta_patreon_fetch` and `patreon_session_cookie` settings): it downloads Jotego's
`jtbeta.zip` and extracts the beta key into `Licenses/`, from where the updater copies it next to
each Jotego beta core.

Where the key comes from, in order:

1. **A file you put on the card.** A `jtbeta.zip` or `beta.bin` in the root of the SD card is used
   as is and nothing is downloaded.
2. **GitHub.** `JTFPGA/jtbeta`, which Jotego's patrons get access to. Needs a GitHub token for an
   account with that access.
3. **Patreon.** The latest Jotego post with a `jtbeta.zip` attachment that your account can view.
   Needs the value of your Patreon `session_id` cookie, copied from your browser while logged in.

## Use

In pupdate (**Pocket Extras → Plugins → Browse plugin catalog...**) or Pocket Sync, add the
plugin from this repository's link, put the SD card in, and run **Jotego Beta Key**.

## Build

```
rustup target add wasm32-wasip1
scripts/build.sh               # target/wasm32-wasip1/release/plugin.wasm
```

## Test

`cargo test` covers the pure logic (config file, campaign id scraping, posts-page parsing, zip
extraction).

Both download paths can be tested offline against the mock server, with the built plugin run
headless by `tools/run_plugin.py` (needs `pip install extism`):

```
python3 tools/mock_jtbeta_server.py --port 8767 --zip some.zip --github-token T --patreon-cookie C &
python3 tools/run_plugin.py target/wasm32-wasip1/release/plugin.wasm /tmp/sd --allow-host 127.0.0.1 \
    --config github_api_url=http://127.0.0.1:8767/ --config patreon_url=http://127.0.0.1:8767/ \
    --answer "GitHub" --answer T
ls /tmp/sd/Licenses /tmp/sd/Plugins/jotego-beta-key
```

## License

MIT (see [LICENSE](LICENSE)).
