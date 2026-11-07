# Jotego Beta Key plugin

A [Pocket plugin](https://github.com/openfpga-library/pocket-plugin) for pupdate and Pocket Sync.
It is the jtbeta auto-fetch that used to live inside pupdate (the `jt_beta_github_fetch`,
`jt_beta_patreon_fetch` and `patreon_session_cookie` settings): it downloads Jotego's
`jtbeta.zip` and extracts the beta key into `Licenses/`, from where the updater copies it next to
each Jotego beta core.

Where the key comes from, in order:

1. **A file you put on the card.** A `jtbeta.zip` or `beta.bin` in the root of the SD card is used
   as is and nothing is downloaded.
2. **GitHub.** `JTFPGA/jtbeta`, which Jotego's sponsors get access to. Needs a GitHub token for an
   account with that access.
3. **Patreon.** The newest Jotego post with a `jtbeta.zip` attachment that your account can view.
   Needs the value of your Patreon `session_id` cookie, copied from your browser while logged in.

On the first run it asks which source to use (GitHub, Patreon, or both with GitHub first) and for
the credential, and keeps them on the SD card in `Plugins/jotego-beta-key/config.json` so they
travel with the card and nothing is asked again. A credential is only saved once it worked; a
rejected one is asked for again, or skipped.

The key is only rewritten when it changed, so the plugin is cheap to run on every **Update All**.
In pupdate, set it to auto-run **before cores** so Jotego's beta cores aren't skipped. Run
unattended (`pupdate update -y`) with nothing configured yet, it cancels its questions and is
reported as needing input.

> The config file holds a credential in plain text on a removable card. Delete
> `Plugins/jotego-beta-key/config.json` if you lend the card or stop using the plugin.

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

The `github_api_url` / `patreon_url` config keys only exist for testing; updaters don't set them.

## When the server can't be reached

Extism ends the plugin run as soon as a request can't connect (an updater shows it as a failed
plugin run). Nothing is written in that case: the plugin only touches the SD card after a
successful download, so just run it again later.

## Release

pupdate and Pocket Sync install the latest release of this repository, which must carry
`plugin.wasm` and `plugin.json`. Publishing a GitHub release runs `.github/workflows/release.yml`,
which builds `plugin.wasm` and attaches both files.

## License

MIT (see [LICENSE](LICENSE)).
