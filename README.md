# copy-sync

使用 server + client 架构同步所有 client 的剪贴板

## Usage

启动 server

```sh
copy-sync create-server --port 5120
```

启动 client
```sh
copy-sync create-client --addr ws://host:5120
```

## TODO

* 处理 ImageData
