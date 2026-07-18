[日本語](#japanese) | [English](#english)

<a id="japanese"></a>

# Herdr TODO

Herdrのワークスペース内でMarkdown形式のTODOを管理するターミナルUIプラグインです。

カレントリポジトリのLocal TODOと、リポジトリ間で共有するGlobal TODOを、Herdrの右サイドペインで同時に確認・編集できます。

本プラグインの制作者は初心者のため、改善・要望いくらでも待ってます。

## 主な機能

- Local TODOとGlobal TODOの2画面表示
- 起動元ペインのcwd変更に追従するLocal TODO
- Markdownチェックリストの読み書き
- 親子関係を持つ階層TODO
- `P1`〜`P3`の優先度
- 優先度→期限が近い順のソート（期限なしは同じ優先度内の末尾）
- TODOごとのカテゴリと、カテゴリ内の最高優先度によるグループ化
- `u`による直前の変更のUndo（Local / Global別、最大100件）
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

## Herdr設定例

`prefix+t`で開く場合は、Herdrの設定ファイルにプラグインアクションを追加します。

macOSとLinuxの既定の設定ファイル：

```text
~/.config/herdr/config.toml
```

設定例：

```toml
[[keys.command]]
key = "prefix+t"
type = "plugin_action"
command = "herdr-todo.open"
description = "Open TODO"
```

設定後はHerdrを再起動するか、次のコマンドで再読み込みします。

```sh
herdr server reload-config
```

この設定では、`prefix+t`を押したペインのcwdがLocal TODOの参照先になります。

起動後も元ペインのcwdを監視します。元ペインで`cd`を実行すると、Local TODOも新しいディレクトリの`TODO.md`へ自動的に切り替わります。TODOの入力・編集中は切り替えを保留し、編集終了後に反映します。

> [!IMPORTANT]
> `type = "shell"`で`herdr plugin pane open`を直接実行しないでください。プラグインアクションを迂回するため、`prefix+t`を押したペインのcwdが引き継がれません。

### 新しいワークスペースで自動起動する（ローカル設定）

このリポジトリの`local-plugin`は、配布用プラグインとは独立したローカル補助プラグインです。リンクすると、新しく作成した各ワークスペースの右側にTODOペインを1つだけ自動で開きます。

```sh
herdr plugin link ./local-plugin
```

無効化・削除する場合（ファイル自体は削除されません）：

```sh
herdr plugin unlink local.herdr-todo-auto-open
```

## 操作方法

| キー                 | 操作                                      |
| -------------------- | ----------------------------------------- |
| `Tab`                | Local / Globalの操作対象を切り替え        |
| `j` / `k`, `↓` / `↑` | TODOを移動                                |
| `Shift+J` / `Shift+K` | 選択TODOを子TODOごと下／上へ並べ替え      |
| `gg` / `G`           | 先頭 / 末尾のTODOへ移動                 |
| `Space`, `Enter`     | 完了状態を切り替え                        |
| `a`                  | TODOを追加                                |
| `e`                  | TODOの本文を編集                          |
| `d`                  | TODOを削除                                |
| `Shift+D`            | 完了済みTODOを子TODOごと一括削除          |
| `Shift+S`            | `SAVE`保護タグを切り替え                  |
| `Shift+C`            | 存在しない場合のみLocal `TODO.md`を作成   |
| `>` / `→`            | 選択中のTODOを子階層にする                |
| `<` / `←`            | 選択中のTODOを親階層へ戻す                |
| `l` / `h`            | Vimライクに子階層 / 親階層へ変更      |
| `p`                  | 優先度を切り替え（未設定 → P1 → P2 → P3） |
| `s`                  | 同じ階層内を優先度→期限が近い順に並べ替え |
| `c`                  | カテゴリを設定・変更（空欄で解除）        |
| `f`                  | カテゴリ別にまとめ、最高優先度順に並べ替え |
| `t`                  | 期限を`YYYY-MM-DD`形式で設定              |
| `u`                  | 直前の変更を元に戻す                      |
| `r`                  | Markdownファイルを再読み込み              |
| `?`                  | ヘルプを表示                              |
| `Cmd+Shift+Q`        | 終了（通常の`q` / `Esc`では閉じません）   |

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

| 種類   | 保存先                                     |
| ------ | ------------------------------------------ |
| Local  | Herdrで開いているワークスペースの`TODO.md` |
| Global | `~/.herdr/TODO.md`                         |

Localの`TODO.md`は起動時に自動作成されません。存在しない場合はLocal画面に案内が表示され、`Shift+C`で新規作成できます。既存ファイルは上書きしません。

```text
TODO.md not found. Create it with Shift+C
```

この表示中もGlobal TODOは通常どおり利用できます。Localを作成する場合のみ`Shift+C`を押してください。

Globalの`~/.herdr/TODO.md`は存在しない場合に自動作成されます。保存先は環境変数で変更できます。

```sh
export HERDR_TODO_GLOBAL_PATH="$HOME/Documents/TODO.md"
```

保存されるMarkdownの例：

```md
# TODO

- [ ] [P1] [SAVE] [CAT:仕事] 今日中に対応 📅 2026-07-18
      複数行の説明も保存できます
  - [x] [P2] 子タスク
- [ ] 通常のタスク
```

優先度や期限が付いていない通常のMarkdownチェックリストも読み込めます。Markdownの見出し・通常文・引用もTODOと同じ一覧に表示します。Markdown表は列幅を揃えた罫線付きの表として表示し、TODOの編集後も元の行を保持します。

カテゴリは`[CAT:カテゴリ名]`として保存されます。`f`を押すと同じ親を持つTODOをカテゴリ別にまとめ、各カテゴリ内で最も高い優先度を基準にカテゴリを並べます。カテゴリなしは末尾です。カテゴリ内の現在の順序と親子構造は維持されます。

Undo履歴は起動中のメモリにLocal / Global別で最大100件保持されます。`r`でMarkdownを再読み込みした場合や、追従元ペインのcwdが変わった場合、そのファイルの履歴はリセットされます。

### 期限と自動削除

| 状態                 | 表示・動作                                 |
| -------------------- | ------------------------------------------ |
| 期限が今日または明日 | 黄色の太字                                 |
| 期限切れ             | 赤色の太字                                 |
| 期限日から7日経過    | 起動または再読み込み時に子TODOごと自動削除 |
| `SAVE`保護中         | 期限から7日経過しても自動削除しない        |

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

---

<a id="english"></a>

# Herdr TODO — English

A terminal UI plugin for managing Markdown TODOs inside a Herdr workspace.

It displays and edits both a Local TODO file for the current project and a Global TODO file shared across projects in a single right-side pane.

## Features

- Side-by-side access to Local and Global TODO lists in a two-section view
- Local TODO tracking when the source pane changes its cwd
- Read and write standard Markdown task lists
- Hierarchical parent and child TODOs
- `P1` through `P3` priorities
- Sorting by priority, then nearest due date; tasks without a due date are placed last within the same priority
- Due dates with overdue tasks shown in bold red
- Tasks due today or tomorrow shown in bold yellow
- Automatic removal of tasks seven days after their due date, with optional `SAVE` protection
- Per-task categories grouped by the highest priority in each category
- Up to 100 in-memory undo entries per Local and Global document
- Bulk deletion of completed tasks
- Multiline popup input
- Display-width-aware wrapping for long text and Japanese characters

## Requirements

- macOS or Linux
- [Herdr](https://herdr.dev) 0.7.0 or later
- A Rust toolchain with `cargo`
- Git

If Rust is not installed, follow the instructions at [rustup](https://rustup.rs/).

## Installation

Install the plugin directly from its GitHub repository:

```sh
herdr plugin install s1223takuma/herdr-todo --yes
```

Verify the installation:

```sh
herdr plugin list --plugin herdr-todo
```

## Opening the TODO pane

Run `Open TODO` from the Herdr plugin actions, or invoke it from the command line:

```sh
herdr plugin action invoke open --plugin herdr-todo
```

The TODO pane opens as a split on the right. The current Herdr split API does not accept a fixed width, so resize it after opening by dragging the border or using Herdr's resize mode. The pane remains open while the Herdr session is active, unless you close it.

The Local TODO path is based on the current directory of the pane from which `Open TODO` is invoked. Select the target project's pane before opening Herdr TODO.

## Herdr configuration example

To open Herdr TODO with `prefix+t`, add a plugin action to the Herdr configuration file.

Default configuration path on macOS and Linux:

```text
~/.config/herdr/config.toml
```

Example:

```toml
[[keys.command]]
key = "prefix+t"
type = "plugin_action"
command = "herdr-todo.open"
description = "Open TODO"
```

Restart Herdr after editing the file, or reload the configuration:

```sh
herdr server reload-config
```

With this configuration, the cwd of the pane in which you press `prefix+t` becomes the Local TODO directory.

Herdr TODO continues monitoring the source pane after opening. If you run `cd` in that pane, the Local section automatically switches to `TODO.md` in the new directory. Cwd changes are deferred while adding or editing a TODO and applied after editing finishes.

> [!IMPORTANT]
> Do not invoke `herdr plugin pane open` directly from a `type = "shell"` binding. That bypasses the plugin action, so the cwd of the pane in which you pressed `prefix+t` is not preserved.

### Auto-open in new workspaces (local setting)

The `local-plugin` directory is a local helper separate from the distributed plugin. Once linked, it automatically opens one TODO pane on the right of each newly created workspace.

```sh
herdr plugin link ./local-plugin
```

To disable and unlink it without deleting its files:

```sh
herdr plugin unlink local.herdr-todo-auto-open
```

## Key bindings

| Key | Action |
| --- | --- |
| `Tab` | Switch the active section between Local and Global |
| `j` / `k`, `↓` / `↑` | Move between TODOs |
| `Shift+J` / `Shift+K` | Move the selected TODO and its children down/up |
| `gg` / `G` | Move to the first / last TODO |
| `Space`, `Enter` | Toggle completion |
| `a` | Add a TODO |
| `e` | Edit TODO text |
| `d` | Delete the selected TODO and its children |
| `Shift+D` | Delete all completed TODOs and their children |
| `Shift+S` | Toggle `SAVE` protection |
| `Shift+C` | Create the Local `TODO.md`, only when it does not exist |
| `>` / `→` | Indent the selected TODO into a child level |
| `<` / `←` | Move the selected TODO toward the root level |
| `l` / `h` | Vim-like indent / outdent |
| `p` | Cycle priority: unset → P1 → P2 → P3 |
| `s` | Sort siblings by priority, then nearest due date |
| `c` | Set or change the category; submit an empty value to clear it |
| `f` | Group categories by the highest priority in each category |
| `t` | Set a due date in `YYYY-MM-DD` format |
| `u` | Undo the last change |
| `r` | Reload the Markdown file |
| `?` | Show help |
| `Cmd+Shift+Q` | Quit (`q` and `Esc` do not close the pane) |

Adding and editing TODOs uses a popup. Press `Shift+Enter` or `Alt+Enter` to insert a newline, `Enter` to save, and `Esc` to cancel.

### Adding a child TODO

1. Place the intended parent at the end of the list, then press `a` to add a TODO.
2. Keep the new TODO selected and press `>` or `→`. The preceding TODO becomes its parent.

```md
- [ ] Parent TODO
  - [ ] Child TODO
```

Deleting a parent also deletes all of its children.

## TODO files

| Scope | Path |
| --- | --- |
| Local | `TODO.md` in the current directory of the pane that opened Herdr TODO |
| Global | `~/.herdr/TODO.md` |

The Local `TODO.md` is not created automatically. When it is missing, the Local section displays a notice. Press `Shift+C` to create it. An existing file is never overwritten by this command.

```text
TODO.md not found. Create it with Shift+C
```

The Global TODO list remains available while this notice is displayed. Press `Shift+C` only when you want to create the Local file.

The Global `~/.herdr/TODO.md` is created automatically when missing. Override its location with an environment variable:

```sh
export HERDR_TODO_GLOBAL_PATH="$HOME/Documents/TODO.md"
```

Example Markdown:

```md
# TODO

- [ ] [P1] [SAVE] [CAT:Work] Finish today 📅 2026-07-18
      Multiline descriptions are supported
  - [x] [P2] Child task
- [ ] Regular task
```

Standard Markdown task lists without priorities or due dates are also supported.

Categories are stored as `[CAT:Category]`. Press `f` to group siblings by category and order categories by the highest-priority TODO they contain. Uncategorized TODOs are placed last. Existing order within each category and parent-child relationships are preserved.

Undo history is kept in memory, separately for Local and Global documents, with a maximum of 100 entries each. Reloading with `r` or changing the tracked source-pane cwd resets the history for that file.

### Due dates and automatic removal

| State | Display or behavior |
| --- | --- |
| Due today or tomorrow | Bold yellow text |
| Overdue | Bold red text |
| Seven days past the due date | Automatically removed with its children on startup or reload |
| Protected with `SAVE` | Not automatically removed after seven days |

When an expired parent is automatically removed, its children are removed as well. Apply `SAVE` protection to the parent with `Shift+S` to preserve the entire hierarchy.

## Updating and uninstalling

Run the installation command again to update. A GitHub-managed plugin checkout is replaced with the latest version:

```sh
herdr plugin install s1223takuma/herdr-todo --yes
```

Uninstall the plugin:

```sh
herdr plugin uninstall herdr-todo
```

Uninstalling the plugin does not delete your Local or Global TODO files.

## Local development

```sh
git clone https://github.com/s1223takuma/herdr-todo.git
cd herdr-todo
cargo test
cargo build --release
herdr plugin link "$PWD"
```

After making changes, rebuild the release binary and relink the manifest when necessary. An already-open TODO pane continues running the old process, so close and reopen it to apply changes. The plugin explicitly launches its own `target/release/herdr-todo`, rather than a command with the same name found on `PATH`.

### Quality checks

```sh
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
```

## Troubleshooting

### The Local TODO path is incorrect

Select the pane for the intended project before invoking `Open TODO`. The working directory of an already-open TODO pane does not change afterward.

### Recent display changes are missing

Build the release binary, relink the plugin, and close and reopen the existing TODO pane:

```sh
cargo build --release
herdr plugin link "$PWD"
```

## Contributing

Bug reports and feature requests are welcome in [GitHub Issues](https://github.com/s1223takuma/herdr-todo/issues). Pull requests are welcome as well.

## License

This project is available under the [MIT License](LICENSE).

Copyright (c) 2026 s1223takuma

[Back to Japanese](#japanese)
