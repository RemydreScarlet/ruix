# Ruix Kernel

RuixはRustで書かれたマイクロカーネルです。Linux互換を長期目標に据えつつ、現在はx86_64上での低レベル機能の実装と安定化に集中しています。

## 目標

Ruixの目的は、最小限のカーネル機能を安全かつ堅牢にRustで実装することです。長期的にはプロセス管理やIPCを含むLinux互換層を構築することを目指します。

## 依存関係

- `bootloader` / `bootimage`（ブート可能イメージ作成、開発用）
- `volatile`（揮発性メモリアクセス）
- `spin`（シンプルなスピンロック）
- `x86_64`（x86_64向け低レベル補助）
- `lazy_static`（静的初期化）

（詳細は `Cargo.toml` を参照してください）

## ビルドと実行

### 前提条件

- Rust（nightlyが必要になる場合があります）
- `bootimage`（ブートイメージを作るなら）:

```bash
cargo install bootimage
```

### ビルド

カーネルをターゲット `x86_64-ruix.json` でビルド:

```bash
cargo build --target x86_64-ruix.json
```

ブート可能イメージを作成する（`bootimage` インストール済みの場合）:

```bash
cargo bootimage
```

### 実行（QEMU）

生成されたブートイメージをQEMUで起動する例:

```bash
qemu-system-x86_64 -drive format=raw,file=target/x86_64-ruix/debug/bootimage-ruix.bin -serial file:serial_output.log
```

## テスト

開発用に簡単なテストやクラッシュ（例：スタックオーバーフロー）を用意してあり、カスタムパニックハンドラの表示を確認できます。自動化されたテストは現在限定的です。

## 貢献

貢献歓迎です。IssueやPRで提案、バグ報告、機能追加の提案を送ってください。実験的なコードが含まれるため、議論を通じて設計を固める流れを推奨します。

## ライセンス

このプロジェクトはMITライセンスの下で公開されています。詳細はLICENSEファイルを参照してください。

## 謝辞

本プロジェクトはOS開発チュートリアルやx86_64クレート、ブートローダー実装から多くを学んでいます。参考資料とコミュニティに感謝します。
