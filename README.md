# circuitgame2-web

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

同じ例は CLI の `circuitgame2 run --input 0110 --input 1000 examples/selected-latch.ncg2` で実行できる。

自分の回路を試すには、ソース欄に `::main` を含む単独の NCG2 ソースを入力して「回路を読み込む」を押す。この画面では入力・出力をそれぞれ 256 ビットまで表示できる。ポートの宣言順と各ポートの最下位ビットからの順序に従い、入力スイッチには 1 からの通し番号を付ける。

[NCG2 の仕様と実装計画](https://neknaj.github.io/circuitgame2/docs/)
