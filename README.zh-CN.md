<p align="center">
  <a href="README.md">English</a> · <a href="README.ru.md">Русский</a> · <b>简体中文</b>
</p>

# mobench

一个跨平台的命令行基准测试工具，用于评估电脑在移动开发（Android / iOS / Flutter）方面的性能。
它只是一个没有任何依赖的可执行文件，运行时不需要 Xcode、Android SDK 或 Flutter。
支持 macOS、Linux 和 Windows，以及 x86_64 和 ARM64 架构。

矩阵乘法之类的纯计算测试很难反映真实的构建速度，因此这里的每个测试都模拟移动应用构建中的一个具体环节。
每个测试都会计算校验和，并且在所有平台上必须完全一致，以此证明每台机器完成的是完全相同的工作。

| 测试 | 模拟的场景 | 线程数 |
|---|---|---|
| `compile_st` | 编译器前端（词法分析 → 语法分析 → 常量折叠 → 类型检查 → 字节码），处理生成的类 Kotlin 代码；模拟单个模块的增量构建 | 1 |
| `compile_mt` | 同上，并行处理 2400 个文件；模拟全量构建（Gradle workers、swift-driver） | 全部 |
| `link_st` | 链接：约 80 万个符号的全局符号表、排序、地址布局、约 190 万次重定位（ld64、lld、R8） | 1 |
| `alloc_mt` | 托管运行时：大量短生命周期对象、指针图、以字符串为键的哈希表（Gradle/Kotlin daemon、Dart VM） | 全部 |
| `compress_mt` | APK/AAB/IPA 打包：带哈希链的 LZ77（deflate 的核心）以及解压校验 | 全部 |
| `hash_mt` | 构建缓存键、代码签名：SHA-256 | 全部 |
| `fs_small` | 构建中间产物：创建、重命名、扫描、读取和删除约 7700 个小文件 | ≤4 |
| `disk_seq` | 顺序写入 1 GiB + fsync | 1 |
| `mem_bw` | 内存带宽（STREAM triad） | 全部 |
| `mem_lat` | 内存延迟（在 128 MiB 上进行指针追逐） | 1 |

## 评分

- 单项得分 = `1000 × 基准时间 / 中位时间`。**1000 分为基准机器（Apple M4 Pro，14 核）**；
  2000 分表示速度是基准的两倍。
- **类别**（Single-core、Multi-core、Memory、Storage）和**平台**（Android、iOS、Flutter）得分
  是各项测试得分的加权几何平均数。权重定义在 `src/main.rs` 的 `PROFILES` 和 `CATEGORIES` 中。
  例如，Android 更看重并行编译和托管运行时（Gradle），iOS 更看重单线程编译和链接器。
- **MOBILE DEV SCORE** 是三个平台得分的几何平均数。

运行结束时，会输出一张汇总卡片，显示硬件信息、操作系统版本和得分：

```
  ╭──────────────────────────────────────────────────────────────────╮
  │  MOBILE DEV SCORE                                           994  │
  │                                                                  │
  │  Device   MacBook Pro (Mac16,7)                                  │
  │  CPU      Apple M4 Pro                                           │
  │  Cores    10P + 4E cores, 14 threads                             │
  │  RAM      24 GB                                                  │
  │  OS       macOS 26.6.2 (build 25G83) · aarch64                   │
  │                                                                  │
  │  Android 997   iOS 993   Flutter 994                             │
  │  Single-core 977   Multi-core 996   Memory 983   Storage 1022    │
  ╰──────────────────────────────────────────────────────────────────╯
```

## 使用方法

```sh
git clone https://github.com/inteloffski/benchpress && cd benchpress
cargo build --release
./target/release/mobench                 # 完整运行：预热 + 3 次测量，M4 Pro 上约 40 秒
./target/release/mobench --quick        # 1 次测量，不预热
./target/release/mobench --dir D:\work  # 在指定磁盘上运行磁盘测试
./target/release/mobench --json result.json
./target/release/mobench --list
```

推送 `v*` 标签时，`.github/workflows/release.yml` 会为所有操作系统和架构构建可执行文件。

## 如何获得可靠的结果

- 笔记本电脑请连接电源（如果检测到使用电池供电，mobench 会给出提示）。
- 关闭 IDE、模拟器和浏览器。
- 如需检查是否存在过热降频，请使用 `--runs 10` 运行，并查看波动范围（`±` 列）。
- 在 Windows 上，`fs_small` 的结果很大程度上受 Defender 影响。这是影响构建速度的真实因素，
  建议与在已加入扫描排除列表的文件夹中运行的结果进行对比。

## 重新校准基准

如果修改了任何测试的工作量，就需要重新校准基准：在基准机器上运行 `mobench --calibrate --runs 5`，
然后把得到的 `ref_secs` 和 `expect` 填入 `src/main.rs` 中的 `TESTS`。
此后旧结果与新结果不再可比，因此请提升版本号。
