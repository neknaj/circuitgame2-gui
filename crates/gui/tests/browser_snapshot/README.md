# ブラウザーのスナップショットを検査する

Ubuntu の GUI チェックでは `_site` の組み立て後にこの検査を実行する。Python 3 と Google Chrome がある Linux 環境では [GUI チェックの手順](../../../../.github/workflows/check.yml)で `_site` を組み立て、次のコマンドで検査できる。

```sh
python3 crates/gui/tests/browser_snapshot/run.py _site
```

検査用ページとファイルは `_site` の一時コピーに置き、`127.0.0.1` だけで配信する。Chrome 上で GUI の WASM を起動し、CLI で保存したスナップショットを復元する。再出力したバイト列が元のファイルと一致することを確認する。短いファイルと長いファイルをそれぞれ拒否し、その後も状態が変わらないことを確かめる。

`cli-snapshot.bin` は160バイトの検査データである。CLI の `088fcc0` をビルドし、このリポジトリのルートで次のコマンドを実行すると再生成できる。

```sh
ncg2 run --input 0110 --save-at 1 crates/gui/tests/browser_snapshot/cli-snapshot.bin site/examples/main.ncg2
```

生成結果の SHA-256 は `478e3148b4a91c1744bd5ae4797f11e82ec557c6538d1281644d554f562f5a65`。
