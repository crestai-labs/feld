# Quickstart

<a href="quickstart.md">English</a> · <a href="quickstart.zh-CN.md">简体中文</a> · 日本語

いつも使っている Claude Code を Feld に attach し、そこから別の Agent を起動し、その Agent に Message を送り、Job を動かします。所要時間は 10 分ほどです。

この手引きは Claude Code と tmux を前提にします。Codex と herdr で異なる点は[末尾](#codex-と-herdr)にあります。各手順には Claude Code に何を頼むかを書きます。Feld の名前を出すのは、Claude Code が `feld` CLI を使うようにするためです。その下の note は、Claude Code が実行するコマンドを説明します。同じコマンドは端末から自分で実行しても構いません。各コマンドのオプションと出力は [CLI リファレンス](cli.md) にあります。

1. [インストール](#インストール)
2. [Claude Code を attach する](#claude-code-を-attach-する)
3. [Agent を起動する](#agent-を起動する)
4. [Message を送る](#message-を送る)
5. [Job を動かす](#job-を動かす)
6. [Web UI](#web-ui)
7. [後片付けとアンインストール](#後片付けとアンインストール)
8. [Codex と herdr](#codex-と-herdr)

## インストール

tmux 3.2 以降と、インストールと認証を済ませた Claude Code 2.1.181 以降が必要です。

Homebrew（macOS、または Homebrew のある Linux）ではこの 1 行です。

```sh
brew install crestai-labs/tap/feld
```

それ以外は install script を使います。このマシン用の Release archive の checksum を確かめて、`feld` と同梱の Plugin を `~/.local/bin` に置きます（置き先は `FELD_INSTALL_DIR` で変えられます）。

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
```

次に Feld を初期設定します。PATH にある Runtime と Provider の Plugin を `~/.feld/plugins/` に置き、Claude Code の設定に Feld の skill と status line を、shell の設定ファイルに `eval "$(feld env)"` を、それぞれ書き込む値を示してから書き、daemon を起動します。

```sh
feld init
```

表示される `next:` の行に従い、新しい shell を開きます。`feld daemon status` に daemon と読み込んだ Plugin が表示されます。

CLI リファレンス：[`feld init`](cli.md#feld-init)、[`feld daemon`](cli.md#daemon)

## Claude Code を attach する

新しい shell から tmux を開き、pane で Claude Code を起動します。

```sh
tmux new-session -s feld
claude
```

Claude Code に頼みます。

```text
lead として Feld に attach して。ほかの Actor を管理する grant も付けて。
```

Feld はこの Claude Code を `lead` Actor の Session として記録し、Claude Code はこれまでどおり使えます。この grant により、`lead` は次の手順でほかの Actor を作り、起動し、止め、読めます。`feld actor get lead` に Session と grant が表示されます。

> [!NOTE]
> Claude Code は次のコマンドを実行します。attach するまで、Claude Code が実行する `feld` CLI は Admin として振る舞います。grant を与えられる principal は Admin だけなので、まず grant 付きで `lead` を宣言し、次にその Actor に attach します。attach した後は、その `feld` コマンドは `lead` として振る舞います。
>
> ```sh
> feld actor create lead --kind agent --runtime claude-code --no-start --grant read --grant control --grant manage-agent --grant manage-job
> feld actor attach lead --self --runtime claude-code
> ```
>
> `/feld attach lead` は grant なしで attach します。後から grant を加えるには、`feld` が再び Admin になる端末から `feld actor update lead --grant read --grant control --grant manage-agent --grant manage-job` を実行します。

CLI リファレンス：[`feld actor create`](cli.md#feld-actor-create)、[`feld actor attach`](cli.md#feld-actor-attach)、[`feld actor update`](cli.md#feld-actor-update)、[grant](../spec/concepts/glossary.md#grants)

## Agent を起動する

`lead` に頼みます。

```text
Feld で、このディレクトリに dev という名前の Claude Code Agent を起動して。
```

Feld は `dev` Actor を作り、その Claude Code を新しい tmux window で起動します。Feld が起動した Claude Code には、skill、hook、telemetry が最初から備わっています。続けて頼みます。

```text
Feld で dev が何をしているか見せて。
```

> [!NOTE]
> Claude Code は次のコマンドを実行します。1 つ目は `dev` Actor を宣言し、現在のディレクトリでその Session を起動します。2 つ目は Session の log を読みます。
>
> ```sh
> feld actor create dev --kind agent --runtime claude-code
> feld session log dev
> ```

Session を終えるには、Feld で `dev` を止めるよう `lead` に頼みます。Feld は `dev` の pane に `/exit` を送ります。`feld actor start dev` は同じ定義で次の Session を起動します。

CLI リファレンス：[`feld actor create`](cli.md#feld-actor-create)、[`feld session log`](cli.md#feld-session-log)、[`feld session stop`](cli.md#feld-session-stop)

## Message を送る

`lead` に頼みます。

```text
Feld で dev に、この repository の構造を review するよう Message で頼んで、返事を見せて。
```

Message は `dev` の次の入力として届き、`dev` は自分の Session の中で答えます。`from` は `lead` です。端末から送った Message の `from` は `Admin` です。

> [!NOTE]
> Claude Code は次のコマンドを実行します。Message を送り、`dev` の log で返事を読み、`dev` の mailbox を一覧します。
>
> ```sh
> feld message send dev "Review the structure of this repository"
> feld session log dev
> feld message list --to dev
> ```

CLI リファレンス：[`feld message`](cli.md#message)

## Job を動かす

`lead` に頼みます。

```text
Feld で、"Hello from Feld" と表示する hello という名前の Job を作って、今すぐ動かして、出力を見せて。
```

Job はコマンドを実行する Actor です。`--` より後がすべてコマンドになり、`--start` はすぐに Session を起動し、出力は Session の log です。`--cron` を付けると、Job は Schedule に従って動きます。

> [!NOTE]
> Claude Code は次のコマンドを実行します。`feld actor get hello` に Session ID が並び、その Session の log が Job の出力です。
>
> ```sh
> feld actor create hello --kind job --start -- sh -c 'printf "Hello from Feld\n"'
> feld actor get hello
> feld session log <session>
> ```

CLI リファレンス：[`feld actor create`](cli.md#feld-actor-create)、[`feld session`](cli.md#session)

## Web UI

Sign in して、既定のブラウザで Web UI を開きます。URL には、5 分間だけ有効な一回限りの Sign in code が入っています。

```sh
feld ui
```

Actor、その Session と Message、Timeline、metrics がすべてそこにあります。各 view が描くものは [Web UI の view](webui.md) にあります。別のデバイスから開くには [`feld ui pair`](cli.md#feld-ui-pair) を見てください。

CLI リファレンス：[`feld ui`](cli.md#feld-ui)

## 後片付けとアンインストール

`lead` に頼みます。

```text
Feld で hello と dev を削除して。
```

履歴は残り、同じ名前はすぐに使い直せます。

> [!NOTE]
> Claude Code は次のコマンドを実行します。Session が動いている Actor は削除できないので、先に `dev` の Session を止めます。
>
> ```sh
> feld session stop dev
> feld actor delete dev
> feld actor delete hello
> ```

daemon は端末から `feld daemon stop` で止めます。

Feld を取り除くには `feld uninstall` を実行します。daemon と Feld が起動した Session を止め、`feld init` が Claude Code と Codex の設定に書いたものを元に戻し、尋ねる前に取り除くものを一覧します。store と `sessions/` は次の `feld init` のために残ります。これらも取り除くには `--purge` を付けます。最後に `next:` の行に従って、shell の設定ファイルから `eval "$(feld env)"` を、そして `feld` の実行ファイル（Homebrew で入れた場合は `brew uninstall feld`）を取り除きます。

CLI リファレンス：[`feld actor delete`](cli.md#feld-actor-delete)、[`feld daemon stop`](cli.md#feld-daemon-stop)、[`feld uninstall`](cli.md#feld-uninstall)

## Codex と herdr

**Codex**（0.156.0 以降）

- `feld init`：一覧で `codex` にチェックを入れます。Codex の config root（`CODEX_HOME`、未設定なら `~/.codex`）に skill と hook を書き、`writable_roots` に Feld の spool を加えます。次に Codex を起動したとき、Feld の hook を信頼するかを尋ねられ、信頼した Codex から連携が有効になります。
- attach：`feld actor attach lead --self --runtime codex` を実行するよう Codex に頼みます。attach は sandbox の外で実行されるので、Codex が尋ねたら承認します。
- 起動：`feld actor create dev --kind agent --runtime codex --workdir "$PWD"`。model や approval policy といった Codex 自身の設定は `--runtime-settings '{"model":"<available-model>","approval_policy":"never"}'` で渡します。終了要求では `/quit` を送ります。
- Message は Codex の次の turn の入力になります。attach した Session の metrics はおおむね `unavailable` です。詳細は [`spec/design/adapter/codex.md`](../spec/design/adapter/codex.md) にあります。

**herdr**（0.9.0 以降）

- `feld init`：herdr server を起動した状態で一覧の `herdr` にチェックを入れ、Feld を使う間は server を動かしたままにします。
- Session は herdr の workspace に置かれます。コマンドは同じで、置き先は `--herdr-session` / `--herdr-unit` / `--herdr-workspace` / `--herdr-pane` / `--herdr-split` で選びます。詳細は [`spec/design/provider/herdr.md`](../spec/design/provider/herdr.md) にあります。
