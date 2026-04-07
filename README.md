### Помошник для QBittorrent и rutracker

[![Clippy Check](https://github.com/andsemenoff/tracker-updater/actions/workflows/clippy.yml/badge.svg)](https://github.com/andsemenoff/tracker-updater/actions/workflows/clippy.yml)
![version](https://img.shields.io/badge/version-v0.1.0-blue)
![Rust Edition](https://img.shields.io/badge/rust-2021-orange?logo=rust)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE-MIT)
![status](https://img.shields.io/badge/status-in%20development-yellow)
[![Contributors](https://img.shields.io/github/contributors/andsemenoff/tracker-updater?style=flat-square)](https://github.com/andsemenoff/tracker-updater/graphs/contributors)

Утилита позволяет определить обновившиеся торренты в qbittorrent на rutracker и пытается их обновить в qbittorrent.

## Что это?

Утилита автоматически обновляет в qbittorrent(через его webUI) торренты, которые были скачены с rutracker и обновились на нем.
Сохряняется предыдущее место установки, поэтому перекачки старых файлов не происходит. Удаляется старый торрент.
Сохраняются категория и все теги. Торренты, которые были удалены с rutracker (нет такой темы, как в комментариях торрента и нет такого хэша) **удаляются из qbittorrent с удалением файлов**.
Все остальные торренты, которые не связаны с rutracker просто пропускаются.

## Как установить?

1. Скачать бинарный файл со страницы GitHub Releases.

## ⚙️ Настройка

Для работы `tracker-updater` требуется конфигурационный файл `config.toml`.

**Важно:** Создайте этот файл в той же папке, куда вы поместили исполняемый файл (`tracker-updater-windows.exe` или `tracker-updater-linux`).
В архиве дистрибутива есть шаблон-пример `config_example.toml` можно просто переименовать его в `config.toml` и изменять данные в нем.

### Основной конфиг (`config.toml`)

Скопируйте этот шаблон в свой файл `config.toml` и заполните его:

```toml
# config.toml

# Режим "пробного запуска".
# true  - Утилита только покажет в логах, что она *собирается* сделать (обновить/удалить),
#         но не будет вносить реальных изменений в qBittorrent.
# false - Утилита будет выполнять реальные обновления и удаления.
dry_run = false

[qbit]
# Адрес вашего qBittorrent WebUI.
# Убедитесь, что WebUI включен в настройках qBittorrent.
url = "http://127.0.0.1:8080"

# Логин и пароль от WebUI
username = "admin"
password = "adminadmin"

[rutracker]
# Ваш сессионный cookie с Rutracker.
# (Инструкцию по получению см. в следующем разделе)
bb_session_cookie = "СЮДА_ВСТАВИТЬ_СКОПИРОВАННОЕ_ЗНАЧЕНИЕ"
```

### 🍪 Как получить `bb_session_cookie`?

Если вы пользуетесь Web-TLO, то см. [следующий раздел](#web_tlo).

Если вы обычный пользователь Rutracker.

`bb_session_cookie` — это ваш ключ аутентификации на Rutracker. Утилита использует его, чтобы скачивать `.torrent` файлы от вашего имени, когда находит обновление.

**Важно:** Никому и никогда не передавайте это значение. Это равносильно передаче логина и пароля от вашего аккаунта.

#### 1. Вход и Инструменты разработчика

-  Откройте `https://rutracker.org` в вашем браузере (Chrome, Firefox, Edge и т.д.).
-  **Войдите в свой аккаунт** (авторизуйтесь). Это обязательный шаг.
-  Нажмите `F12`, чтобы открыть **Инструменты разработчика**. (Или `Ctrl+Shift+I`, или `Cmd+Opt+I` на macOS).

#### 2. Поиск cookie

-  В открывшейся панели перейдите на вкладку:
    * **Chrome / Edge / Яндекс Браузер:** `Application` (Приложение)
    * **Firefox:** `Storage` (Хранилище)
-  В меню слева найдите раздел `Cookies` и кликните на `https://rutracker.org`.
-  В таблице появится список. Найдите в колонке `Name` (Имя) строчку **`bb_session`**.
-  Скопируйте значение из колонки `Value` (Значение) для этой строки. Это длинная строка из букв и цифр.

#### 3. Вставка в `config.toml`

-  Вставьте скопированное значение в ваш `config.toml` (который вы создаете в той же папке, где лежит бинарный файл):

    ```toml
    # config.toml
    
    dry_run = false
    
    [qbit]
    url = "http://127.0.0.1:8080"
    username = "admin"
    password = "adminadmin"
    
    [rutracker]
    bb_session_cookie = "СЮДА_ВСТАВИТЬ_СКОПИРОВАННОЕ_ЗНАЧЕНИЕ"
    ```
<a id="web_tlo"></a>    
### Как получить `bb_session_cookie` если вы пользуетесь Web-TLO?

Если вы используете Web-TLO, то в папке `web-tlo\nginx\wtlo\data` нужно найти файл `config.ini`
В этом файле найти раздел [torrent-tracker] и в нем строчку 
`user_session="bb_session=....."`
Из этой строчки копируем только часть, которая у меня обозначена как .....

Вставьте скопированное значение в ваш `config.toml` (который вы создаете в той же папке, где лежит бинарный файл):

```toml
# config.toml

dry_run = false

[qbit]
url = "http://127.0.0.1:8080"
username = "admin"
password = "adminadmin"

[rutracker]
bb_session_cookie = "СЮДА_ВСТАВИТЬ_СКОПИРОВАННОЕ_ЗНАЧЕНИЕ"
```

## 🚀 Как использовать

### 1. Предварительные требования

1.  **qBittorrent WebUI:** Убедитесь, что в настройках qBittorrent включен WebUI (Веб-интерфейс) и вы знаете его адрес, логин и пароль.
2.  **Комментарии к торрентам:** Утилита находит ID раздачи **только** из комментария к торренту. Убедитесь, что у ваших раздач с Rutracker в qBittorrent есть комментарий, содержащий ссылку вида:
    `https://rutracker.org/forum/viewtopic.php?t=1234567`

### 2. Запуск

1.  Скачайте последнюю версию (`rutracker-updater-windows.exe`, `rutracker-updater-linux` и т.д.) со страницы [Релизы](https://github.com/andsemenoff/rutracker-updater/releases).
2.  Создайте папку в удобном месте (например, `C:\Tools\RutrackerUpdater`).
3.  Поместите скачанный **исполняемый файл** в эту папку.
4.  Рядом с ним создайте и настройте файлы `config.toml` и (по желанию) `log4rs.yaml` (инструкции по настройке см. в разделе "Настройка").
5.  Просто **запустите исполняемый файл** (двойным кликом на Windows или `./rutracker-updater-linux` в терминале Linux).

### 3. Первый запуск (Рекомендуется)

Перед полноценным использованием **настоятельно рекомендуется** сделать пробный запуск.

1.  Установите в `config.toml` флаг `dry_run = true`.
2.  Запустите утилиту.
3.  Откройте файл лога (по умолчанию `logs/my_app.log`).
4.  Изучите лог. Вы увидите сообщения `[INFO]` или `[WARN]` о том, какие торренты были бы обновлены или удалены, с пометкой `🟢 DRY-RUN`.
5.  Если все выглядит корректно, установите `dry_run = false` в `config.toml` для боевого режима.

### 4. Как это работает?

При запуске утилита:
1.  Подключается к вашему qBittorrent.
2.  Находит все торренты, у которых в поле "Трекер" указан `rutracker`.
3.  Для каждого торрента извлекает ID из его комментария.
4.  Проверяет через API Rutracker, изменился ли хеш для этого ID.
5.  **Если хеш изменился (раздача обновлена):**
    * Скачивает новый `.torrent` файл.
    * Добавляет его в qBittorrent, **указывая тот же путь сохранения**, что был у старого торрента. (qBittorrent автоматически начнет перепроверку файлов).
    * Удаляет *старый* торрент из qBittorrent, **не удаляя файлы**.
6.  **Если торрент удален с Rutracker (API вернул `null`):**
    * Удаляет торрент из qBittorrent, **включая скачанные файлы**.

## Сборка из исходного кода

Если вы предпочитаете не использовать готовые бинарные файлы из [Релизов](https://github.com/andsemenoff/tracker-updater/releases), вы можете собрать проект из исходного кода.

### Требования

1.  **Git:** Необходим для клонирования репозитория.
2.  **Rust:** Необходима установленная среда Rust (через [rustup](https://rustup.rs/)). Проект использует издание (edition) 2021.

### Шаги сборки

1.  Клонируйте репозиторий:
    ```sh
    git clone [https://github.com/andsemenoff/tracker-updater.git](https://github.com/andsemenoff/tracker-updater.git)
    cd tracker-updater
    ```

2.  Соберите проект в режиме "release" (это оптимизирует исполняемый файл):
    ```sh
    cargo build --release
    ```
    *Эта команда загрузит все зависимости и скомпилирует проект.*

3.  После завершения сборки, исполняемый файл будет находиться в папке `target/release/`:
    * `target/release/tracker-updater` (для Linux/macOS)
    * `target/release/tracker-updater.exe` (для Windows)

### Запуск после сборки

**ВАЖНО:** Исполняемый файл для работы ожидает найти конфигурационные файлы в той же папке, откуда он запущен.

1.  Создайте папку, где будет "жить" ваша утилита (например, `C:\Tools\TrackerUpdater` или `~/bin/tracker-updater`).
2.  Скопируйте туда собранный исполняемый файл из `target/release/`.
3.  Скопируйте `log4rs.yaml` и `config_example.toml` из корня репозитория в вашу новую папку.
4.  Переименуйте `config_example.toml` в `config.toml`.
5.  Настройте `config.toml`, как описано в разделе "Настройка".
6.  Запустите исполняемый файл.

## Планы
- Работа с несколькими qbittorrent клиентами
- Обработка ошибок авторизации

## Тестирование

- **Модульные тесты** (быстрая проверка внутренней логики парсинга и форматов):
  ```sh
  cargo test --lib
  ```

- **Интеграционные тесты** (тестирование основной логики через реальный qBittorrent и API) 
  ```bash
  cargo test --test test_update_logic -- --ignored --nocapture
  ``` 
  при этом устаревшие торренты лежат в tests/test-files. Они должны называться `old1.torrent` и `old2.torrent`. Они не содержатся в проекте на github.
- **Тесты структуры API qBittorrent:** 
    ```bash
    cargo test --test test_qbit_api_fields -- --ignored --nocapture
    ```

## Разработчикам

Для запуска workflow handmade_realise.yml запускаем в ручную и указываем новый тег
<!-- 
- Создаем локальный тег командой `git tag v0.1.0`
- Отправляем этот тег на GitHub командой `git push origin v0.1.0`
-->

## 💡 Похожие проекты

Существует несколько проектов, решающих схожие задачи. Если моя утилита вам не подошла, возможно, вам пригодится:

* **[konkere/TorrUpd](https://github.com/konkere/TorrUpd)**: Утилита со схожим функционалом, написанная на Python. Она также умеет проверять актуальность торрентов (включая Rutracker по хешу) и автоматически обновлять их в qBittorrent и Transmission.

## Лицензия

Этот проект лицензирован **на ваш выбор** (at your option) по одной из следующих лицензий:

* **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE) или http://www.apache.org/licenses/LICENSE-2.0)
* **MIT license** ([LICENSE-MIT](LICENSE-MIT) или http://opensource.org/licenses/MIT)