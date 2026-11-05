# 官方更新发布工具

仅维护者执行，学校教室不保存发布私钥，也不运行本工具。依赖固定在 `update-publisher-requirements.txt`；签名、TUF模型及签名字节序列化全部使用官方 `python-tuf` / `securesystemslib`，Rust接收端使用独立维护的 `sigstore-tuf` 验证。

```sh
python3 -m venv /维护者私有目录/update-venv
/维护者私有目录/update-venv/bin/pip install -r scripts/update-publisher-requirements.txt
/维护者私有目录/update-venv/bin/python scripts/publish-update-metadata.py init-root \
  --key-dir /维护者私有目录/future-classroom-signing \
  --output /公开发布目录/1.root.json
```

`--key-dir`必须不存在，且位于代码仓库之外；工具创建0700目录、0600私钥。初始root是Ed25519的2/3阈值，Targets、Snapshot、Timestamp各使用独立密钥。初始化完成后将三个root私钥分开离线保管，日常发布目录只保留三个在线角色私钥和发布序号记录。公开root必须随可信初装版本编入，不能从同伴、扫码网页、离线更新封套替换初始信任。不要用测试fixture的root部署产品。

先通过既有正式安装包构建流程生成字节完整的 `Setup.exe` / `FutureClassroom.msi`，再建立 `payload.zip`（固定名称，禁止开发用GNU Showcase ZIP）。`custom.json`格式见 `fc-protocol/src/update_release.rs`；精确前驱必须来自真实正式安装源，不能凭版本号填写。`releaseSequence`始终增长，显示版本号不是安全序号。在线角色元数据版本使用该序号；工具保留维护者私有高水位，拒绝回用序号。

```sh
/维护者私有目录/update-venv/bin/python scripts/publish-update-metadata.py publish \
  --root /公开发布目录/1.root.json \
  --key-dir /维护者私有目录/future-classroom-signing \
  --payload /公开发布目录/payload.zip \
  --claims /公开发布目录/custom.json \
  --output-dir /公开发布目录/metadata
python3 scripts/package-update-envelope.py create \
  --payload /公开发布目录/payload.zip \
  --metadata-dir /公开发布目录/metadata \
  --output /公开发布目录/FutureClassroom.fcupdate
```

每次输出到新目录/新文件，不覆盖已有交付物。TUF签名对象是外部角色文件里的原始 `payload.zip` hash，不能把封套hash填回自身。Targets和Snapshot有效期一年、Timestamp九十天、初始Root五年；离线学校需要在到期前补充新的已签名元数据。更新元数据过期只停止接收新更新，不停止教学软件。

`package-update-envelope.py`只检查传输结构；签名发布工具检查发布格式；接收端独立验TUF、完整hash、ZIP白名单及原Setup/MSI绑定。真正安装还需原生安装器检查已签来源、精确前驱、Windows当前用户及运行lease，不能用任一工具成功代替完整安装资格。

测试公共fixture位于 `crates/fc-bootstrap/tests/fixtures/updates/`，内容明确标记 `TEST-ONLY-NOT-INSTALLABLE`。其私钥已销毁；它仅证明Python官方TUF输出能被Rust验证、缓存和恢复，不能证明Windows安装成功。
