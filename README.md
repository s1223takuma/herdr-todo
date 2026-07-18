# Herdr TODO

Herdrのワークスペース内でMarkdown形式のTODOを管理するターミナルUIプラグインです。

カレントリポジトリのLocal TODOと、リポジトリ間で共有するGlobal TODOを、Herdrの右サイドペインで同時に確認・編集できます。

## 主な機能

- Local TODOとGlobal TODOの2画面表示
- Markdownチェックリストの読み書き
- 親子関係を持つ階層TODO
- `P1`〜`P3`の優先度
- 優先度→期限が近い順のソート（期限なしは同じ優先度内の末尾）
- 期限の設定と期限切れの赤色表示
- 期限が今日または明日のTODOを黄色の太字で警告
- 期限から7日経過したTODOの自動削除と`SAVE`保護
- 完了済みTODOの一括削除
- 複数行入力のポップアップ
- 長文と日本語の表示幅を考慮した折り返し

## 必要環境

- macOSまたはLinux
- [Herdr](https://herdr.dev) 0.7.0以上
- Rustツールチェーン（`cargo`）
- Git

Rustが未導入の場合は、[rustup](https://rustup.rs/)の手順に従ってインストールしてください。

## インストール

HerdrのプラグインコマンドからGitHubリポジトリを指定します。

```sh
herdr plugin install s1223takuma/herdr-todo --yes
```

インストール状態は次のコマンドで確認できます。

```sh
herdr plugin list --plugin herdr-todo
```

## 起動

Herdr内のプラグインアクションから`Open TODO`を実行します。CLIから開く場合は次の通りです。

```sh
herdr plugin action invoke open --plugin herdr-todo
```

TODOペインはHerdrの右側にsplitとして開きます。現在のHerdrではsplitに幅を直接指定できないため、起動後に境界のドラッグまたはresize modeで調整してください。Herdrのセッションが維持されている間は、ペインも閉じるまで残ります。

Local TODOの参照先は、`Open TODO`を実行した元ペインのカレントディレクトリです。対象プロジェクトのペインを選択してから起動してください。

## 操作方法

| キー | 操作 |
| --- | --- |
| `Tab` | Local / Globalの操作対象を切り替え |
| `j` / `k`, `↓` / `↑` | TODOを移動 |
| `Space`, `Enter` | 完了状態を切り替え |
| `a` | TODOを追加 |
| `e` | TODOの本文を編集 |
| `d` | TODOを削除 |
| `Shift+D` | 完了済みTODOを子TODOごと一括削除 |
| `Shift+S` | `SAVE`保護タグを切り替え |
| `Shift+C` | 存在しない場合のみLocal `TODO.md`を作成 |
| `>` / `→` | 選択中のTODOを子階層にする |
| `<` / `←` | 選択中のTODOを親階層へ戻す |
| `p` | 優先度を切り替え（未設定 → P1 → P2 → P3） |
| `s` | 同じ階層内を優先度→期限が近い順に並べ替え |
| `t` | 期限を`YYYY-MM-DD`形式で設定 |
| `r` | Markdownファイルを再読み込み |
| `?` | ヘルプを表示 |
| `q`, `Esc` | 終了 |

TODOの追加・編集はポップアップで行います。`Shift+Enter`または`Alt+Enter`で改行し、`Enter`で保存します。

### 子TODOの追加

1. 親にしたいTODOが一覧の末尾にある状態で、`a`でTODOを追加します。
2. 追加したTODOを選択したまま`>`または`→`を押すと、直前のTODOの子になります。

```md
- [ ] 親TODO
  - [ ] 子TODO
```

親TODOを削除すると、その子TODOも一緒に削除されます。

## TODOファイル

| 種類 | 保存先 |
| --- | --- |
| Local | Herdrで開いているワークスペースの`TODO.md` |
| Global | `~/.herdr/TODO.md` |

Localの`TODO.md`は起動時に自動作成されません。存在しない場合はLocal画面に案内が表示され、`Shift+C`で新規作成できます。既存ファイルは上書きしません。

Globalの`~/.herdr/TODO.md`は存在しない場合に自動作成されます。保存先は環境変数で変更できます。

```sh
export HERDR_TODO_GLOBAL_PATH="$HOME/Documents/TODO.md"
```

保存されるMarkdownの例：

```md
# TODO

- [ ] [P1] [SAVE] 今日中に対応 📅 2026-07-18
      複数行の説明も保存できます
  - [x] [P2] 子タスク
- [ ] 通常のタスク
```

優先度や期限が付いていない通常のMarkdownチェックリストも読み込めます。

### 期限と自動削除

| 状態 | 表示・動作 |
| --- | --- |
| 期限が今日または明日 | 黄色の太字 |
| 期限切れ | 赤色の太字 |
| 期限日から7日経過 | 起動または再読み込み時に子TODOごと自動削除 |
| `SAVE`保護中 | 期限から7日経過しても自動削除しない |

親TODOが自動削除対象の場合は子TODOも削除されます。階層ごと残したい場合は親TODOを`Shift+S`で保護してください。

## アップデートとアンインストール

アップデートは同じインストールコマンドを再実行します。GitHubから導入したプラグインは最新の管理チェックアウトで置き換えられます。

```sh
herdr plugin install s1223takuma/herdr-todo --yes
```

アンインストール：

```sh
herdr plugin uninstall herdr-todo
```

## ローカル開発

```sh
git clone https://github.com/s1223takuma/herdr-todo.git
cd herdr-todo
cargo test
cargo build --release
herdr plugin link "$PWD"
```

変更後は再度`cargo build --release`を実行し、必要に応じて`herdr plugin link "$PWD"`でmanifestを再読み込みします。

現在開いているTODOペインは古いプロセスのままなので、変更を反映するには一度閉じて`Open TODO`から開き直してください。プラグインはPATH上の同名コマンドではなく、プラグイン内の`target/release/herdr-todo`を起動します。

### 品質チェック

```sh
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
```

## トラブルシューティング

### Local TODOの参照先が意図と違う

参照したいプロジェクトのペインを選択してから`Open TODO`を実行してください。すでに開いているTODOペインのcwdは後から変更されません。

### 変更した表示が反映されない

Releaseビルドと再リンク後、既存のTODOペインを閉じて開き直してください。

```sh
cargo build --release
herdr plugin link "$PWD"
```

## コントリビューション

不具合報告や改善案は[GitHub Issues](https://github.com/s1223takuma/herdr-todo/issues)へお願いします。Pull Requestも歓迎します。

## ライセンス

このプロジェクトは[MIT License](LICENSE)で公開されています。

Copyright (c) 2026 s1223takuma
