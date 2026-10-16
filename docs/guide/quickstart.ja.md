# Quickstart

<a href="quickstart.md">English</a> · <a href="quickstart.zh-CN.md">简体中文</a> · 日本語

Feld をインストールし、tmux 上の Claude Code を Feld から扱います。所要時間は 10 分ほどです。

この手引きは Claude Code と tmux を前提にします。Codex と herdr で異なる点は、該当する箇所に記します。各節のコマンドのオプションと出力は [CLI リファレンス](cli.md) に、ほかの利用者向け文書は [Guide](README.md) に、contract の正本は [`docs/spec/`](../spec/) にあります。

1. [前提条件](#前提条件)
2. [インストール](#インストール)
3. [初期設定](#初期設定)
4. [手動で起動した Claude Code を attach する](#手動で起動した-claude-code-を-attach-する)
5. [Feld から Claude Code を起動する](#feld-から-claude-code-を起動する)
6. [Message を送る](#message-を送る)
7. [Job を作って動かす](#job-を作って動かす)
8. [Web UI](#web-ui)
9. [後片付け](#後片付け)
10. [アンインストール](#アンインストール)

## 前提条件

- tmux 3.2 以降（herdr 0.9.0 以降）
- Claude Code 2.1.181 以降（Codex 0.156.0 以降）。インストールと認証を済ませておく

## インストール

Homebrew（macOS、または Homebrew のある Linux）ではこの 1 行です。

```sh
brew install crestai-labs/tap/feld
```

それ以外は install script を使います。GitHub の Release からこのマシン用の archive を取り、checksum を確かめて、`feld` と同梱の Plugin を `~/.local/bin` に置きます。置き先は `FELD_INSTALL_DIR`、版は `FELD_VERSION` で選べます。

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
```

`~/.local/bin` が PATH に無ければ加えます。source からの build は [CONTRIBUTING.md](../../CONTRIBUTING.md#development-setup) にあります。

## 初期設定

端末で `feld init` を実行し、質問に答えます。`FELD_HOME`（既定は `~/.feld`）を作り、同梱の Plugin から選んだものを `~/.feld/plugins/` に置き、Claude Code と shell の設定を書き、daemon を起動します。`--yes` はスクリプト向けで、何も聞かずにすべてに同意します。初回は端末で答え、何を書き換えるかを確かめてください。

```sh
feld init
```

1. **Plugin を選ぶ**：`↑` `↓` で項目を移動し、`Space` でチェックを切り替え、`Enter` で確定します。PATH にある Runtime（`claude`）と Provider（`tmux`）の Plugin は、最初からチェックされています
2. **Claude Code の設定**：`claude-code` Runtime Adapter は、Claude Code の config root（`CLAUDE_CONFIG_DIR`、未設定なら `~/.claude`）の `skills/feld/` に Feld の skill と Claude Code plugin を書き、同じ root の `settings.json` にある `otelHeadersHelper` と `statusLine` をそこへ向けます。この 2 つについては、現在の値と書き込む値を示して同意を求めます。自前の `statusLine` があれば、そのコマンドは Feld の status line helper の `--chain` に残り、status line には Feld の区画と並んで自前の出力が表示されます。自前の `otelHeadersHelper` は置き換えられ、元の値は `~/.feld/prepared/claude-code.json` に保存されます
3. **shell の設定**：shell の設定ファイル（zsh なら `~/.zshrc`、bash なら `~/.bashrc`）に `eval "$(feld env)"` を追記します。この行を読み込んだ shell から起動した Claude Code は、daemon の Collector に telemetry を送ります
4. **daemon の起動**：daemon は Control API と Runtime API、Web UI、OTel Collector を起動し、`~/.feld/plugins/` の Plugin を読み込みます

完了すると、`next:` に順に実行するコマンド、`ui:` に Web UI の URL、`note:` に Runtime ごとの注意が表示されます。`next:` に従って、設定ファイルを `source` するか、新しい shell を開きます。

daemon の状態は次のコマンドで確かめます。`runtimes:` と `providers:` に読み込んだ Plugin が表示されます。止まっている daemon は `feld daemon start` で起動します。環境を見直すには `feld doctor` を使います。検査ごとの結果と直し方を 1 行ずつ表示します。

```sh
feld daemon status
```

> [!NOTE]
> **Codex**：一覧で `codex` にもチェックを入れます。`codex` Runtime Adapter は、config root（`CODEX_HOME`、未設定なら `~/.codex`）に skill と hook の宣言を書き、`config.toml` の `writable_roots` に Feld home の `spool/shared` を加えます。次に Codex を起動したとき、Feld の hook を信頼するかを尋ねられ、信頼した Codex から連携が有効になります。認証は Codex 自身の login で行います。sandbox と spool の詳細は [`spec/design/adapter/codex.md`](../spec/design/adapter/codex.md) にあります
>
> **herdr**：herdr server を起動した状態で、一覧の `herdr` にチェックを入れます。herdr API は動いている server にしか届かないので、Feld を使う間は server を動かしたままにします。以降のコマンドは同じで、Session は herdr の workspace に置かれます。どの server のどの unit に置くかは `--herdr-session` / `--herdr-unit` / `--herdr-workspace` / `--herdr-pane` / `--herdr-split` で指定します
>
> **スクリプトから置く**：答えは `--runtime` / `--provider` で渡します。同じ種類を 2 つ置くには、flag を 2 回渡します。Plugin を変えた `feld init` は、動いている daemon を再起動するかを尋ねます

CLI リファレンス：[`feld init`](cli.md#feld-init)、[`feld daemon`](cli.md#daemon)、[`feld doctor`](cli.md#feld-doctor)、[launchSpec オプション](cli.md#launchspec-options)

## 手動で起動した Claude Code を attach する

`eval "$(feld env)"` を読み込んだ shell から tmux を開き、pane で Claude Code を起動します。

```sh
tmux new-session -s feld-claude
claude
```

次のコマンドを実行するよう Claude Code に頼みます。Claude Code の中で `/feld attach` を実行すると自動生成した名前で attach し、`/feld attach <ACTOR>` なら `<ACTOR>` として attach します。

```sh
feld actor attach manual-claude --self --runtime claude-code
```

Feld は、動いている Claude Code を `manual-claude` Actor の attach した Session として記録します。Claude Code の起動と終了は引き続き利用者が行います。skill、hook、Plugin Monitor は `feld init` の後に起動した Claude Code で有効になり（すでに開いている Claude Code では `/reload-plugins` の後）、telemetry は `feld init` の後に起動した Claude Code で有効になります。この準備が無い Claude Code も attach でき、出力の `note:` 行が `feld init` を案内します。`feld env` の無い shell から起動した Claude Code では、attach の出力の `telemetry` 行に metrics が `unavailable` と理由付きで表示されます。

別の端末から attach の結果を確かめます。

```sh
feld actor get manual-claude
```

> [!NOTE]
> **Codex**：`feld actor attach manual-codex --self --runtime codex` を実行するよう Codex に頼みます。attach は Admin の操作なので、sandbox の外で実行されます。sandbox の中の `feld` は拒否され、Codex は sandbox の外で実行する承認を求めます。承認すると attach が通ります。承認を求めない Codex（`approval_policy` が `never`）では、拒否に添えられたコマンド（Codex を表示している pane を指します）を sandbox の外の shell で実行します。attach した Session の metrics は、hook が送る context window の使用率を除いて `unavailable` です

CLI リファレンス：[`feld actor attach`](cli.md#feld-actor-attach)

## Feld から Claude Code を起動する

`claude-dev` Agent を作ります。作成時に、Agent は tmux 上で Claude Code を起動します。

```sh
feld actor create claude-dev --kind agent --runtime claude-code --workdir "$PWD"
```

起動した Claude Code には、`claude-code` Runtime Adapter が Session ごとの設定と Claude Code plugin を用意し、skill、hook、Plugin Monitor、telemetry の設定を起動時から渡します。Session の会話は次のコマンドで読みます。

```sh
feld session log claude-dev
```

Feld が起動した Session の終了は、次のコマンドで要求します。

```sh
feld session stop claude-dev
```

Feld は Claude Code の pane に `/exit` を送り、タスクが動いていれば終了確認への答えも送って、Session が終わります。`feld actor start claude-dev` は次の Session を起動し、`feld actor restart claude-dev` は現在の Session を止めて次の Session に置き換えます。attach した Session を終えるには、attach した Claude Code 自体を終了します。

> [!NOTE]
> **Codex**：`--runtime codex` で作ります。model は Codex の既定で、model や approval policy といった Codex 自身の設定は、`config.toml` の項目として `--runtime-settings` に渡します。`<available-model>` には、Codex を起動して `/model` に並ぶ名前を使います。終了要求では、Feld は Codex の pane に `/quit` を送ります
>
> ```sh
> feld actor create codex-dev --kind agent --runtime codex --workdir "$PWD"
> feld actor update codex-dev --runtime-settings '{"model":"<available-model>","approval_policy":"never"}'
> feld actor restart codex-dev
> ```
>
> Codex は、`config.toml` で trusted と記された workdir ではすぐに入力を待ち、それ以外の workdir では信頼の確認を表示して待ちます。workdir の `.codex/` の設定と hook が確認なしに有効になってよい repo では、`--trust-workdir` でそれを宣言します
>
> ```sh
> feld actor create codex-dev --kind agent --runtime codex --workdir "$PWD" --trust-workdir
> ```

CLI リファレンス：[`feld actor create`](cli.md#feld-actor-create)、[`feld session log`](cli.md#session)、[`feld session stop`](cli.md#feld-session-stop)、[launchSpec オプション](cli.md#launchspec-options)

## Message を送る

別の端末から Claude Code に Message を送ります。宛先は、attach した `manual-claude` でも、Feld が起動した `claude-dev` でも構いません。

```sh
feld message send claude-dev "Review the structure of this repository"
```

Plugin Monitor が有効な Claude Code では、Message は Session に自動で届きます。`feld init` より前から開いている Claude Code など、Plugin Monitor がまだ有効でない Session では、次のコマンドを実行して Message を受け取るよう Claude Code に頼みます。

```sh
feld self message recv
```

外の端末から Actor の mailbox を確かめます。外の端末から送った Message の `from` は `Admin` です。

```sh
feld message list --to claude-dev
```

> [!NOTE]
> **Codex**：queue helper が Message を Codex の queue に入れ、次の turn の入力になります

CLI リファレンス：[`feld message`](cli.md#message)、[`feld self`](cli.md#self)

## Job を作って動かす

`hello` Job を作り、すぐに起動します。

```sh
feld actor create hello --kind job --start -- sh -c 'printf "Hello from Feld\n"'
```

`--` より後がすべて Job のコマンドになります。Job は `--start` を付けたときだけ、作成時に Session を起動します。Job の定義と Session は `default` Workspace に記録されます。

Actor の一覧、Job の詳細、Feld が残した record を確かめます。

```sh
feld actor list -a
feld actor get hello
feld record list --actor hello
```

Session ID は `feld actor get` の `Sessions` に並びます。Session ID を渡して Job の出力を読みます。

```sh
feld session log <session>
```

同じ定義で新しい Session を起動するには、次を実行します。

```sh
feld actor start hello
```

`feld session get` の `spawned_by Admin` と `feld record list` の `by Admin` は、この端末からの操作を指します。daemon の local token を使った CLI の操作は [Admin](../spec/concepts/glossary.md#admin) として記録されます。Admin は Actor の外にいる principal で、Message の宛先になれるのは mailbox を持つ Actor だけです。

`--cron` を付けると Schedule を持つ Job になり、`feld actor pause` / `feld actor resume` で Schedule を止めたり再開したりします。

```sh
feld actor create nightly --kind job --cron '0 9 * * *' --timezone Asia/Tokyo -- ./nightly.sh
```

CLI リファレンス：[`feld actor`](cli.md#actor)、[`feld session`](cli.md#session)、[`feld record`](cli.md#record)

## Web UI

Sign in して、既定のブラウザで Web UI を開きます。

```sh
feld ui
```

URL だけを表示するには、次を実行します。

```sh
feld ui --print-url
```

URL の fragment には一回限りの Sign in code が入っています。そのブラウザで 5 分以内に開いてください。

daemon は、loopback に送られた要求と、`--allow-host` で許可した名前宛ての要求にだけ応答します。別のデバイスのブラウザから開くには、reverse proxy で daemon の Port を中継し、その host 名を許可して daemon を起動します（`feld daemon restart --allow-host <host>`）。そのブラウザで開いた Web UI の Sign in 画面で、daemon のマシンで `feld ui pair` が表示する code を入力します。

```sh
feld ui pair --name laptop
```

> [!NOTE]
> reverse proxy には Tailscale を想定しています。`tailscale serve --bg <PORT>` で daemon の Port を中継し、`tailscale serve status` が表示する名前（`<host>.<tailnet>.ts.net`）を `--allow-host` で許可します。

Sign in したブラウザは `feld browser list` で一覧し、`feld browser revoke laptop` で 1 つを Sign out します。Settings の Sign out は、そのブラウザ自身を Sign out します。

各 view が描くものは [Web UI の view](webui.md) にあります。

CLI リファレンス：[`feld ui`](cli.md#feld-ui)

## 後片付け

使い終えた Actor を削除します。履歴は残り、同じ名前はすぐに使い直せます。現在の Session が動いている Actor は、先に `feld session stop` で止めます。

```sh
feld actor delete hello
```

daemon を止めます。

```sh
feld daemon stop
```

CLI リファレンス：[`feld actor delete`](cli.md#feld-actor-delete)、[`feld daemon stop`](cli.md#feld-daemon-stop)

## アンインストール

Feld を取り除くには `feld uninstall` を実行します。daemon と Feld が起動した Session を止め、`feld init` が Claude Code と Codex の config root に書いたものを元に戻し、Feld home から設定を取り除きます。尋ねる前に、取り除くものを一覧します。

```sh
feld uninstall
```

`sessions/` と store は残るので、もう一度 `feld init` を実行すれば同じ履歴を読めます。これらも取り除くには `--purge` を付けます。最後に `next:` の行に従って、shell の設定ファイルから `eval "$(feld env)"` を、そして `feld` の実行ファイル（Homebrew で入れた場合は `brew uninstall feld`）を取り除きます。

CLI リファレンス：[`feld uninstall`](cli.md#feld-uninstall)
