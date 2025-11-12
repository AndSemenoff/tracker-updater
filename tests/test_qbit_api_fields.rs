// tests/test_qbit_api_fields.rs

use qbit_rs::{model::Credential, Qbit};

// Добавляем импорты для config
use config::{Config as ConfigBuilder, File};
use tracker_updater::Config; // Наша структура Config из lib.rs

/// ИНТЕГРАЦИОННЫЙ ТЕСТ ДЛЯ АНАЛИЗА ПОЛЕЙ QBITTORRENT
///
/// Этот тест подключается к qBittorrent, используя учетные данные из config.toml,
/// берет ПЕРВЫЙ торрент из списка и выводит в консоль ВСЕ
/// доступные для него поля из двух основных структур:
///   1. `TorrentInfo` (из общего списка `get_torrent_list`)
///   2. `TorrentProperties` (из детального запроса `get_torrent_properties`)
///
/// ЗАПУСК:
/// 1. Убедись, что qBittorrent запущен.
/// 2. Убедись, что в нем есть хотя бы один торрент.
/// 3. Убедись, что файл config.toml существует и настроен.
/// 4. Выполни в терминале:
///    cargo test --test test_qbit_api_fields -- --ignored --nocapture
///
#[tokio::test]
#[ignore] // Этот тест - интеграционный, он требует живого подключения.
async fn dump_all_torrent_fields() {
    // --- 1. Загружаем config.toml ---
    println!("Загрузка config.toml...");

    let builder =
        ConfigBuilder::builder().add_source(File::with_name("config.toml").required(true));

    let config_settings = builder
        .build()
        .expect("Ошибка загрузки config.toml. Убедитесь, что он существует.");

    let config: Config = config_settings
        .try_deserialize::<Config>()
        .expect("Ошибка парсинга config.toml.");

    // Извлекаем нужные данные
    let url = config.qbit.url;
    let username = config.qbit.username;
    let password = config.qbit.password;

    println!("Подключение к {}...", url);

    // --- 2. Инициализация клиента ---
    let credential = Credential::new(username, password);
    let client = Qbit::new(url.as_str(), credential);

    // --- 3. Получаем список торрентов ---
    let torrents_result = client.get_torrent_list(Default::default()).await;

    let torrents = match torrents_result {
        Ok(torrents) => torrents,
        Err(e) => {
            panic!(
                "❌ Ошибка подключения или получения списка: {}. \
                   Убедитесь, что qBittorrent запущен и config.toml файл корректен.",
                e
            );
        }
    };

    if torrents.is_empty() {
        println!("⚠️ В qBittorrent нет ни одного торрента. Нечего проверять.");
        return;
    }

    // --- 4. Берем ПЕРВЫЙ торрент из списка ---
    let first_torrent_info = &torrents[0];
    let hash = match first_torrent_info.hash.as_ref() {
        Some(h) => h,
        None => {
            println!(
                "⚠️ Первый торрент ('{}') не имеет хеша. Пропускаем.",
                first_torrent_info.name.as_deref().unwrap_or("?")
            );
            return;
        }
    };

    println!(
        "\n✅ Успешно! Анализируем торрент: '{}'",
        first_torrent_info.name.as_deref().unwrap_or("?")
    );
    println!("  Хеш: {}", hash);

    // --- 5. ВЫВОДИМ ВСЕ ПОЛЯ ИЗ TorrentInfo (get_torrent_list) ---
    //
    // !!! СКОРЕЕ ВСЕГО, `magnet_uri` БУДЕТ ПРЯМО ЗДЕСЬ !!!
    //
    println!("\n--- 1. Поля из `TorrentInfo` (ответ get_torrent_list) ---");
    // println!("{:#?}", first_torrent_info);
    println!("Magnet: {:#?}", first_torrent_info.magnet_uri);

    // --- 6. ПОЛУЧАЕМ И ВЫВОДИМ ВСЕ ПОЛЯ ИЗ TorrentProperties ---
    match client.get_torrent_properties(hash).await {
        Ok(properties) => {
            println!("\n--- 2. Поля из `TorrentProperties` (ответ get_torrent_properties) ---");
            // `TorrentProperties` содержит такие вещи, как список трекеров,
            // комментарий, дату создания и т.д.
            println!("{:#?}", properties);
        }
        Err(e) => {
            println!(
                "\n❌ Не удалось получить TorrentProperties для хеша {}: {}",
                hash, e
            );
        }
    }

    println!("\n--- Готово ---");
    println!("Ищи поле `magnet_uri` в выводе `TorrentInfo` выше.");
}
