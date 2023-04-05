# copy-sync

跨平台剪贴板同步程序，支持文本和图像数据。

## Usage

启动 server

```sh
copy-sync start --port 5120
```

启动 client

```sh
copy-sync connect --addr ws://host:5120
```
