# circuitgame2-gui

NCG2 の順序回路をブラウザーで実行する。回路の解析と tick の実行には、CLI と同じ Rust の Core を使う。

## 例題を実行する

Git、Rustup、Python 3 を用意する。このリポジトリで次のコマンドを実行し、ブラウザーで [localhost:8000](http://localhost:8000/) を開く。

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo build --locked --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir site/pkg target/wasm32-unknown-unknown/debug/circuitgame2_web.wasm
python -m http.server --bind 127.0.0.1 --directory site 8000
```

付属の例題は 4 ビットの入力から 2 ビットを選び、ラッチに保持する。

1. 読み込み直後の出力は tick 0 の `00`。
2. 入力スイッチのビット 2 と 3 をオンにして「1 tick 進める」を押す。入力 `0110` に対する出力は `01`。
3. ビット 1 だけをオンにして、もう一度「1 tick 進める」を押す。入力 `1000` に対して出力は `01` のまま。

同じ例は CLI の `ncg2 run --input 0110 --input 1000 examples/selected-latch.ncg2` で実行できる。

自分の回路を試すには、ソース欄に `::main` を含む単独の NCG2 ソースを入力して「回路を読み込む」を押す。この画面では入力・出力をそれぞれ 256 ビットまで表示できる。ポートの宣言順と各ポートの最下位ビットからの順序に従い、入力スイッチには 1 からの通し番号を付ける。

## 複数ファイルのプロジェクトを開く

「circuitgame2.toml があるフォルダー」で、設定ファイルと回路ソースが入ったフォルダーを選ぶ。設定の `sources` に列挙したファイルを読み、`entry` が指すモジュールを実行する。設定と回路ソースは UTF-8 で保存する。

CLI リポジトリの `examples/project` で試せる。読み込み直後の出力は 1 ビットの `0`。スイッチをオフにしたまま 1 tick 進めると出力は `1` になる。

### RAM の記憶を読む

CLI リポジトリの `examples/vram` を開くと、公開 VRAM の `screen` が表示される。スイッチのビット 1、2、4 をオンにして 1 tick 進めると、回路の出力は `00`、更新後の RAM の記憶部は `0010` になる。書き込んだ tick の回路出力には、書き込み前の値が現れる。

記憶部はアドレス順に並び、各語を最下位ビットから表示する。128 ビットを超える RAM は「前の範囲」「次の範囲」で表示位置を切り替える。

外部デバイスと NCG2M の実行は未対応。

[NCG2 の仕様と実装計画](https://neknaj.github.io/circuitgame2/docs/)

## 共通 GUI の試作

`circuitgame2-gui` クレートの画面は、ネイティブとブラウザーで同じ実装を使う。起動すると付属の回路が開く。ソースを編集して読み込み直し、入力スイッチ、リセット、1 tick 実行を試せる。

ネイティブで起動する。

```sh
cargo run --locked -p circuitgame2-gui
```

ブラウザーで試すには、先に上の手順で `wasm-bindgen` を用意し、次のコマンドを実行して [localhost:8000](http://localhost:8000/) を開く。

```sh
cargo build --locked -p circuitgame2-gui --lib --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir crates/gui/web/pkg target/wasm32-unknown-unknown/debug/circuitgame2_gui.wasm
python -m http.server --bind 127.0.0.1 --directory crates/gui/web 8000
```

`circuitgame2.toml` を使う場合は、画面の「Open project folder」で設定ファイルがあるフォルダーを選ぶ。「Project source」で編集するファイルを切り替え、「Load circuit」で全ファイルの編集を反映する。構文エラーが出たときは「Go to error」で該当箇所を選択できる。編集中の内容は、そのプロジェクトを開いている間、メモリーに保持する。CLI リポジトリの [`examples/project`](https://github.com/neknaj/circuitgame2-cli/tree/main/examples/project) で試せる。

公開した RAM は「VRAM」に表示される。記憶部はアドレス順に並び、各語を最下位ビットから表示する。128 ビットを超える記憶部は「Previous」「Next」で表示範囲を切り替える。CLI リポジトリの [`examples/vram`](https://github.com/neknaj/circuitgame2-cli/tree/main/examples/vram) で試せる。

「Waveform」では、各 tick に渡した入力と回路が返した出力から、表示するビットを選べる。直近の最大 128 サンプルを保持する。リセットすると、tick 0 の初期値から記録を始める。

ブラウザーでは、同梱の M PLUS 1p を日本語のコメントと診断の表示に使う。OS の IME を使った入力は確認中。

フォントの[配布元](https://github.com/google/fonts/tree/d714b17ce2379f06daf6295617f961df605dccb5/ofl/mplus1p)と[ライセンス](crates/gui/web/fonts/OFL.txt)を参照。
