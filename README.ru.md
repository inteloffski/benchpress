<p align="center">
  <a href="README.md">English</a> · <b>Русский</b> · <a href="README.zh-CN.md">简体中文</a>
</p>

# mobench

Кроссплатформенный CLI-бенчмарк компьютера для мобильной разработки (Android / iOS / Flutter).
Один бинарник без зависимостей: не нужен ни Xcode, ни Android SDK, ни Flutter. Работает на
macOS, Linux и Windows, x86_64 и ARM64.

Вместо «попугаев» вроде умножения матриц каждый тест моделирует конкретный этап мобильной сборки.
Каждый тест считает контрольную сумму, и она должна совпадать на всех платформах: так видно, что
все машины выполнили одну и ту же работу.

| тест | что моделирует | потоки |
|---|---|---|
| `compile_st` | фронтенд компилятора (lexer → parser → const-fold → type-check → bytecode) на сгенерированном Kotlin-подобном коде: инкрементальная сборка одного модуля | 1 |
| `compile_mt` | то же на 2400 файлах параллельно: clean build (Gradle workers, swift-driver) | все |
| `link_st` | линковка: глобальная таблица ~800k символов, сортировка, раскладка адресов, ~1.9M релокаций (ld64, lld, R8) | 1 |
| `alloc_mt` | managed runtime: множество короткоживущих объектов, графы указателей, хэш-мапы со строковыми ключами (Gradle/Kotlin daemon, Dart VM) | все |
| `compress_mt` | упаковка APK/AAB/IPA: LZ77 с hash chains (ядро deflate) + проверочная распаковка | все |
| `hash_mt` | ключи build cache, codesign: SHA-256 | все |
| `fs_small` | build intermediates: создание, rename, сканирование, чтение и удаление ~7.7k мелких файлов | ≤4 |
| `disk_seq` | последовательная запись 1 GiB + fsync | 1 |
| `mem_bw` | пропускная способность памяти (STREAM triad) | все |
| `mem_lat` | латентность памяти (pointer chasing по 128 MiB) | 1 |

## Очки

- Очки теста = `1000 × эталонное_время / медиана_времени`. **1000 — эталон (Apple M4 Pro, 14 ядер)**;
  2000 — в два раза быстрее.
- **Категории** (Single-core, Multi-core, Memory, Storage) и **платформы** (Android, iOS, Flutter)
  считаются как взвешенное геометрическое среднее тестов. Веса заданы в `PROFILES` и `CATEGORIES`
  в `src/main.rs`. Например, для Android больше весят параллельная компиляция и managed runtime
  (Gradle), для iOS — однопоточная компиляция и линковщик.
- **MOBILE DEV SCORE** — геометрическое среднее трёх платформ.

В конце прогона выводится итоговая карточка с железом, версией ОС и очками:

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

## Запуск

```sh
git clone https://github.com/inteloffski/benchpress && cd benchpress
cargo build --release
./target/release/mobench                 # полный прогон: прогрев + 3 замера, ~40 с на M4 Pro
./target/release/mobench --quick        # 1 замер без прогрева
./target/release/mobench --dir D:\work  # дисковые тесты на нужном диске
./target/release/mobench --json result.json
./target/release/mobench --list
```

Бинарники под все ОС и архитектуры собирает `.github/workflows/release.yml` (на push тега `v*`).

## Как получить честные результаты

- Ноутбук должен быть на зарядке (бенчмарк предупредит, если он на батарее).
- Закройте IDE, эмуляторы, браузер.
- Для оценки троттлинга запустите `--runs 10` и посмотрите на разброс (колонка `±`).
- На Windows `fs_small` сильно зависит от Defender. Это реальный фактор скорости сборки,
  поэтому сравните результат с папкой, добавленной в исключения.

## Пересчёт эталона

Если меняется объём работы любого теста, эталон пересчитывается: `mobench --calibrate --runs 5`
на эталонной машине, затем полученные `ref_secs` и `expect` вписываются в `TESTS` в `src/main.rs`.
После этого старые результаты несравнимы с новыми, поэтому поднимите версию.
