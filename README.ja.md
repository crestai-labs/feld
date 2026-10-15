<p align="center">
  <img src="docs/assets/feld-logo-light.png#gh-light-mode-only" alt="Feld" width="160" />
  <img src="docs/assets/feld-logo-dark.png#gh-dark-mode-only" alt="Feld" width="160" />
</p>

<h1 align="center">Feld</h1>

<p align="center"><strong>Agent Loop Control Plane</strong></p>

<p align="center">
  <a href="https://github.com/crestai-labs/feld/actions/workflows/ci.yml"><img src="https://github.com/crestai-labs/feld/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI" /></a>
  <a href="https://github.com/crestai-labs/feld/releases/latest"><img src="https://img.shields.io/github/v/release/crestai-labs/feld?sort=semver" alt="Release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License: Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey" alt="Platform: macOS | Linux" />
</p>

<p align="center">
  <a href="#対応製品">対応製品</a> · <a href="#インストール">インストール</a> · <a href="#はじめる">はじめる</a> · <a href="docs/guide/quickstart.md">Quickstart</a> · <a href="#ドキュメント">ドキュメント</a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">简体中文</a> · 日本語
</p>

<p align="center">
  <img src="docs/assets/readme/actors-light.png#gh-light-mode-only" alt="Feld の Web UI。Actor ごとの Session の timeline と metrics が並ぶ" width="960" />
  <img src="docs/assets/readme/actors-dark.png#gh-dark-mode-only" alt="Feld の Web UI。Actor ごとの Session の timeline と metrics が並ぶ" width="960" />
</p>

---

Feld は、複数の Coding Agent をループで動かす開発者のための、Harness 非依存な **Agent Loop Control Plane** です。それぞれの Session を見える状態に保ち、Agent 同士の連携と人による制御を支えます。

> [!NOTE]
> Feld はまだ実験的な段階にあります。CLI、API、store の形は、0.x の間に互換性なく変わることがあります（[互換性の約束](docs/guide/compatibility.md)）。

## Why Feld?

複数の Agent や定期ジョブを組み合わせてループや Harness（あるいは Software Factory）を構築すると、よく同じところでつまずきます。

- 止まった Agent が、まだ動いているように見える。
- cron や launchd の定期ジョブが、いつの間にか黙って止まっている。
- 前回の実行が終わる前に次が起動し、同じ仕事を二重に始める。
- 止まっている Agent に送った依頼が、届かずに消える。
- Claude Code と Codex では、起動の仕方も状態の見え方も違う。

どれも、「誰が動いているか」「いつ何が起きたか」を tmux や cron、自作のメッセージの仕組みの状態から推測していることが原因です。

Feld はこの部分を引き受けます。Agent と Job に恒久的な identity を与え、Session の起動と終了、Message の配送、定期実行の発火と見送りを、事実として一つの永続的な timeline に記録します。止まっている Agent 宛ての Message は、次の Session が読みます。Claude Code と Codex は、同じ identity と Message の仕組みで扱えます。人は Web UI と CLI から、何が動いていて、どこに対応が必要かを見て、次を決められます。

次にどの Agent を動かすか、いつ打ち切るかといった方針は、利用者の Harness に残ります。Harness は自由に選べ、Feld はその下で共通の control plane になります。

## Feld ではないもの

- **Coding Agent ではありません。** Feld 自身はコードを書かず、Claude Code や Codex を置き換えません。仕事は Coding Agent が行い、Feld は登録された Session をまたぐ永続的な identity、履歴、Message を提供します。
- **Harness、Agent framework、LLM SDK ではありません。** Feld は model、prompt、tool、plan を決めず、次にどの Agent が動くかも決めません。利用者が Harness と Runtime を持ち込み、Feld はその周囲に共通の control plane を提供します。
- **log viewer ではありません。** log は Feld が扱う情報の一部です。Feld は lifecycle record、Message、metrics、Feld-owned Session の制御も一つにまとめます。

## 仕組み

<p align="center">
  <img src="docs/assets/readme/how-it-works-ja-light.png#gh-light-mode-only" alt="Feld の仕組み。CLI と Web UI は Feld daemon を使い、daemon は tmux や herdr の中の Coding Agent（Claude Code / Codex）を起動・停止し、Message を届け、transcript と metrics を受け取る" width="360" />
  <img src="docs/assets/readme/how-it-works-ja-dark.png#gh-dark-mode-only" alt="Feld の仕組み。CLI と Web UI は Feld daemon を使い、daemon は tmux や herdr の中の Coding Agent（Claude Code / Codex）を起動・停止し、Message を届け、transcript と metrics を受け取る" width="360" />
</p>

中心にいるのは Feld daemon です。人や Harness は CLI と Web UI から daemon を使います。Coding Agent は tmux や herdr の中で、いつもの姿のまま動きます。daemon はその Session を起動・停止し、Message を届け、Agent から transcript と metrics を受け取って一つの timeline に記録します。

| 概念 | 意味 |
|---|---|
| Actor | 名前を持つ恒久的な主体。次の 2 つがある。<ul><li>Agent（Claude Code / Codex）</li><li>Job（任意のコマンド。cron で定期実行もできる）</li></ul> |
| Session | Actor の一回の実行。次の 2 つがある。<ul><li>Feld が起動したもの</li><li>すでに動いている Agent に取り付けた（attach した）もの</li></ul> |
| Message | Actor 同士、人と Actor の間で交わす依頼や報告。止まっている Actor 宛ては次の Session が読む |

詳しい関係と用語は[用語集](docs/spec/concepts/glossary.md)にあります。

## 対応製品

| 種類 | 製品 | 対応版 |
|---|---|---|
| OS | macOS（Apple Silicon） | — |
| | Linux（x86_64） | glibc 2.35 以降 |
| 実行環境 | tmux | 3.2 以降 |
| | [herdr](https://herdr.dev) | 0.9.0 以降 |
| Coding Agent | Claude Code | 2.1.181 以降 |
| | Codex | 0.156.0 以降 |

実行環境と Coding Agent は、それぞれ 1 つ以上が入っていれば動きます。Coding Agent はインストールと認証を済ませておいてください。

## インストール

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
```

GitHub の Release からこのマシン用の archive を取り、checksum を確かめて `~/.local/bin` に `feld` と同梱の Plugin を置きます。置き先は `FELD_INSTALL_DIR`、版は `FELD_VERSION` で変えられます。source からの build は [CONTRIBUTING.md](CONTRIBUTING.md#development-setup) にあります。

## アップグレード

```sh
curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
feld init
```

install script をもう一度実行すると、実行ファイルが新しい版に置き換わります。`feld init` をもう一度実行すると、新しい Plugin を `~/.feld/plugins/` に置き、Claude Code と Codex の設定にある Feld の skill と hook を置き換え、daemon を再起動します。daemon は起動時に store を自動で移行し、Session は再起動の間も動き続けるので、Agent を止めずに版を上げられます。詳しくは [互換性](docs/guide/compatibility.md#upgrading) にあります。

## はじめる

```sh
feld init
feld ui
```

`feld init` で初期設定を行い、`feld ui` でブラウザに Web UI を開きます。

例えば Claude Code を Feld に接続するには、新しい shell から tmux か herdr の pane で `claude` を起動し、次を実行します。

```text
/feld attach
# あるいは「あなた自身を Feld にアタッチして」と頼む
```

続きは [Quickstart](docs/guide/quickstart.md) にあります。

## 使い方

```sh
# Feld から Agent を起動する（Codex なら --runtime codex）
feld actor create developer-01 --kind agent --runtime claude-code --workdir "$PWD"

# Message を送る
feld message send developer-01 "変更を作り、reviewer-01 にレビューを依頼し、結果を報告せよ"

# cron で動く Job を作る
feld actor create dispatcher --kind job --cron "0 * * * *" -- python3 dispatch.py

# 状態と履歴を見る
feld actor list
feld session log developer-01
feld record list --actor dispatcher
```

すべてのコマンドとオプションは [CLI リファレンス](docs/guide/cli.md) にあります。アンインストールするときは `feld uninstall` を実行します。

## FAQ

**対応している OS は？**
macOS（Apple Silicon）と Linux（x86_64）です。詳しくは[対応製品](#対応製品)にあります。

**Windows で使えますか？**
WSL2 の中で使えます。Native 対応は検討中です。

**Docker image はありますか？**
ありません。daemon は host で動かします。

**データを外部に送りますか？**
送りません。利用統計も送りません。Feld が読むもの、書くものは [Data and privacy](docs/guide/data-and-privacy.md) にあります。

**Claude Code や Codex を置き換えるものですか？**
置き換えません。Feld はそれらを動かしたまま、周りに control plane を足します。

## ドキュメント

| 文書 | 内容 |
|---|---|
| [Quickstart](docs/guide/quickstart.md) | 最初に読む手引き |
| [CLI リファレンス](docs/guide/cli.md) | コマンド、オプション、出力 |
| [互換性](docs/guide/compatibility.md) | 版の上げ方と、0.x の間の互換性の約束 |
| [Data and privacy](docs/guide/data-and-privacy.md) | Feld が送らないもの、読むもの、書くもの、消し方 |
| [`docs/spec/`](docs/spec/) | 仕様の正本（用語集、CLI、daemon API、Web UI、設計） |
| [CONTRIBUTING.md](CONTRIBUTING.md) | source からの build、テスト、Issue と PR の出し方 |

## License

Feld は [Apache License 2.0](LICENSE) で提供します。商用を含めて自由に使え、改変と再配布ができます。Apache-2.0 を選んだのは、特許の利用許諾が明記されており、企業でも安心して採用できるためです。

Kazuki Yoshida が作り、CrestAI が開発と保守を行っています。

著作権表示は [NOTICE](NOTICE) を、商標の扱いは [TRADEMARK.md](TRADEMARK.md) を参照してください。

配布する archive には、同梱する第三者のソフトウェアとそのライセンスの一覧（`THIRD-PARTY-LICENSES.md`）を添えています。
