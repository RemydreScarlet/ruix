# Ruix Kernel

RuixはRustで書かれたマイクロカーネルです。Linux互換を長期目標に据えつつ、現在はx86_64上での低レベル機能の実装と安定化に集中しています。

## 目標

Ruixの目的は、最小限のカーネル機能を安全かつ堅牢にRustで実装することです。長期的にはプロセス管理やIPCを含むLinux互換層を構築することを目指します。

## 現在の機能（主要な実装箇所）

- VGAテキスト出力と基本的な `print!` / `println!` 機能（`src/vga_buffer.rs`）。
- Global Descriptor Table (GDT) と Task State Segment の初期化（`src/gdt.rs`）。
- Interrupt Descriptor Table (IDT) と例外ハンドラ（`src/interrupts.rs`）。
- カスタムパニックハンドラ（画面出力）。
- メモリ管理の基礎（`src/memory.rs`）とアロケータ実装（`src/allocator.rs`、`src/allocator/fixed_size_block.rs`）。
- 簡易なタスク実行環境（`src/task/executor.rs`、`src/task/keyboard.rs`、`src/task/mod.rs`）。
- システムコールの枠組み（`src/syscall.rs`）。

## プロジェクト構造（主なファイル）

- [src/main.rs](src/main.rs): カーネルのエントリーポイント（`_start`）。
- [src/lib.rs](src/lib.rs): ライブラリ層、初期化シーケンスの公開。 
- [src/vga_buffer.rs](src/vga_buffer.rs): VGAテキストバッファの実装。
- [src/gdt.rs](src/gdt.rs): GDT/TSS セットアップ。
- [src/interrupts.rs](src/interrupts.rs): IDT と例外ハンドラ。
- [src/memory.rs](src/memory.rs): メモリ管理ユーティリティ。
- [src/allocator.rs](src/allocator.rs) と [src/allocator/fixed_size_block.rs](src/allocator/fixed_size_block.rs): アロケータ実装。
- [src/syscall.rs](src/syscall.rs): システムコール処理の基礎。
- [src/task](src/task): タスク周りの実装（executor, keyboard など）。
- Cargo.toml: 依存関係とビルド設定。
- x86_64-ruix.json: カスタムターゲット仕様（x86_64ベアメタル）。

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
