# circuitgame2-gui

NCG2 の順序回路を実行する GUI。回路の解析と tick の実行には、CLI と同じ Rust の Core を使う。

## ブラウザーで使う

[共通 GUI](https://neknaj.github.io/circuitgame2-gui/) を開くと、付属の回路を編集し、入力を切り替え、手動または連続で実行できる。複数ファイルの回路を実行するには、「Open project folder」で `circuitgame2.toml` のあるフォルダーを選ぶ。

## 共通 GUI をローカルで実行する

`circuitgame2-gui` クレートの画面は、ネイティブとブラウザーで同じ実装を使う。起動すると付属の回路が開く。ソースを編集して読み込み直し、入力スイッチを操作し、画面上部の Run で連続実行、Pause で一時停止、Step で1 tick 実行できる。Reset を押すと停止して tick 0 に戻る。ticks/s は画面上の実行ペースを設定する。シミュレーションの論理 tick 周期は変わらない。

ネイティブで起動する。

```sh
cargo run --locked -p circuitgame2-gui
```

ブラウザーで試すには、次のコマンドを実行して [localhost:8000](http://localhost:8000/) を開く。

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo build --locked -p circuitgame2-gui --lib --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir crates/gui/web/pkg target/wasm32-unknown-unknown/debug/circuitgame2_gui.wasm
python -m http.server --bind 127.0.0.1 --directory crates/gui/web 8000
```

`circuitgame2.toml` を使う場合は、画面の「Open project folder」で設定ファイルがあるフォルダーを選ぶ。「Project source」で編集するファイルを切り替え、「Load circuit」で全ファイルの編集を反映する。構文エラーが出たときは「Go to error」で該当箇所を選択できる。編集中の内容は、そのプロジェクトを開いている間、メモリーに保持する。CLI リポジトリの [`examples/project`](https://github.com/neknaj/circuitgame2-cli/tree/main/examples/project) で試せる。

公開した RAM は「VRAM」に表示される。記憶部はアドレス順に並び、各語を最下位ビットから表示する。128 ビットを超える記憶部は「Previous」「Next」で表示範囲を切り替える。CLI リポジトリの [`examples/vram`](https://github.com/neknaj/circuitgame2-cli/tree/main/examples/vram) で試せる。

「Waveform」には、入力と出力をポート名付きで並べる。共通の tick 目盛で信号の変化を比べ、カーソルを重ねた tick の値を各行で確認できる。「Ticks shown」で直近の表示範囲を調整する。履歴は最大128サンプルで、リセットすると tick 0 の初期値から記録し直す。

ブラウザーでは、同梱の M PLUS 1p を日本語のコメントと診断の表示に使う。OS の IME を使った入力は確認中。

フォントの[配布元](https://github.com/google/fonts/tree/d714b17ce2379f06daf6295617f961df605dccb5/ofl/mplus1p)と[ライセンス](crates/gui/web/fonts/OFL.txt)を参照。

## スナップショットから再開する

順序回路を開き、ネイティブでは「Save snapshot…」、ブラウザーでは「Download snapshot」を押す。操作した時点の確定 tick、適用済みの入力、ゲート、ラッチ、RAM の状態をファイルに保存する。入力スイッチを変更してまだ tick を進めていない場合、保存するのは最後に回路へ適用した入力である。

再開するときは同じ回路を読み込み、「Restore snapshot」でファイルを選ぶ。配線や設定の tick 周期が一致すると、保存時の tick・入出力・RAM を表示し、以降の波形をそこから記録する。「Step」で次の tick を実行できる。

ファイル形式は CLI の `ncg2 run --save-at` と `--restore` と共通。同じ回路と tick 周期ならネイティブ、ブラウザー、CLI の間でファイルを渡せる。[CLI の保存と復元](https://github.com/neknaj/circuitgame2-cli#readme)

## デスクトップ GPU の回路実行

デスクトップ版では Vulkan に対応した GPU を使う。`circuitgame2-gpu` の Rust API は Core の検査済み配線を受け取る。`GpuNor32` は複数の入力列で同じ順序 NOR 回路を実行し、`GpuCombinationalNor32` は複数の入力で同じ組み合わせ NOR 回路を評価する。組み合わせ回路では1回のディスパッチで公開出力を計算する。

デスクトップ版を起動し、次の回路をソース欄に貼り付ける。

```lisp
(using nor)
(func main
  (in (a (bits 1)))
  (out (q (bits 1)))
  (def q (nor a a)))
```

「GPU batch」の入力欄に `0` と `1` を別々の行に書き、「Run on GPU」を押す。各行が1ケースにあたる。「Case」で表示する出力を選ぶ。

順序回路を試すには、上の `func` を `module` に変える。同じ行の中で tick ごとの入力を空白で区切り、例えば `0 1 0` と `1 0 1` を別々の行に書く。全ケースの tick 数をそろえ、「Case」と「Tick」で表示する出力を選ぶ。tick 0 はリセット直後の出力を示す。

入力と出力はポートの宣言順に並び、各ポートのビットは最下位ビットから数える。入力ポートがない場合は各入力に `-` を書く。GPU バッチでは標準のラッチ、RAM、外付け I/O を使う回路を扱えない。

実機テストでは両方の GPU API の出力を CPU 版と比較する。順序回路については、複数回の GPU 投入をまたぐ tick 順と最終状態も確認する。

```sh
cargo test --locked -p circuitgame2-gpu -- --ignored
```

[GPU と RP マイコンの設計](https://neknaj.github.io/circuitgame2/docs/backends.html)

## 従来のブラウザー版をローカルで実行する

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
