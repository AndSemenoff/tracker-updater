// src/lib.rs

//!
//! Модуль для работы с торрентами и API rutracker.cc
//!

pub mod rutracker_api;
pub mod torrent;

use qbit_rs::{
    model::{AddTorrentArg, Credential, TorrentFile, TorrentSource},
    Qbit,
};
use rutracker_api::{
    extract_torrent_id_from_comment, get_api_peer_stats_by_hash_async,
    get_api_torrent_hash_by_id_async,
};
use std::collections::HashMap;
use std::error::Error;
use torrent::Torrent;

use reqwest::header::{HeaderMap, HeaderValue, COOKIE, USER_AGENT};
use reqwest::Client;
use serde::Deserialize; // <-- ДОБАВЛЕНО для config
use tokio::fs;

/// Инициализация логгера с использованием log4rs
pub fn init_logger() {
    match log4rs::init_file("log4rs.yaml", Default::default()) {
        Ok(_) => log::debug!("log4rs.yaml загружен, логгер инициализирован."),
        Err(e) => log::error!(
            "❌ Ошибка инициализации log4rs (файл log4rs.yaml не найден?): {}",
            e
        ),
    }
}

/// Конфигурация qBittorrent клиента
#[derive(Deserialize, Debug)]
pub struct QbitConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

/// Конфигурация Rutracker API
#[derive(Deserialize, Debug)]
pub struct RutrackerConfig {
    pub bb_session_cookie: String,
}

/// Структура конфигурации для запуска хелпера
/// #[derive(Deserialize)] автоматически реализует загрузку из файла
#[derive(Deserialize, Debug)]
pub struct Config {
    pub dry_run: bool,
    pub qbit: QbitConfig,
    pub rutracker: RutrackerConfig,
}

/// Главная функция-координатор логики (ранее была в main.rs)
///
/// Принимает структуру конфигурации и выполняет всю работу.
pub async fn run_helper(config: Config) -> Result<(), Box<dyn Error>> {
    // 1. Настройка клиентов на основе конфига

    // Клиент для скачивания .torrent файлов
    let client_for_download = Client::builder().build()?;

    // Создаем заголовки
    let mut headers = HeaderMap::new();
    // Используем новое поле из конфига
    let cookie_string = format!("bb_session={}", config.rutracker.bb_session_cookie);
    headers.insert(COOKIE, HeaderValue::from_str(&cookie_string)?);
    headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/100.0.0.0 Safari/537.36"));

    log::info!("Подключение к {}...", config.qbit.url);

    // Клиент qBittorrent
    let credential = Credential::new(config.qbit.username.clone(), config.qbit.password.clone());
    let client = Qbit::new(config.qbit.url.as_str(), credential);

    // 2. Запуск основного процесса
    // Прокидываем флаг dry_run в основную логику
    if let Err(e) = process_torrents(&client, &client_for_download, &headers, config.dry_run).await
    {
        log::error!("❌ Ошибка при обработке торрентов: {}", e);
        log::error!("Это также может быть ошибкой входа (неверный пароль) или подключения.");
        log::error!("Убедитесь, что qBittorrent запущен и учетные данные верны.");
        return Err(e);
    }

    Ok(())
}

// -----------------------------------------------------------------
// --- ВСЕ ОСТАЛЬНЫЕ ФУНКЦИИ (приватные, без `pub`) ---
// -----------------------------------------------------------------

/// Координатор: получает данные, сравнивает, запускает обновление/удаление
async fn process_torrents(
    client: &Qbit,
    client_for_download: &Client,
    headers: &HeaderMap,
    dry_run: bool,
) -> Result<(), Box<dyn Error>> {
    // 1. Получаем список наших торрентов из qBittorrent
    let mut my_torrents = match get_qbit_torrents(client).await {
        Ok(torrents) => torrents,
        Err(e) => {
            log::error!("❌ Ошибка при получении списка торрентов: {}", e);
            return Err(e);
        }
    };

    if my_torrents.is_empty() {
        log::info!("Торрентов c Rutracker не найдено. Завершение работы.");
        return Ok(());
    }

    log::info!(
        "Найдено {} торрентов с Rutracker. Поиск обновлений...",
        my_torrents.len()
    );

    // 2. Обновляем статистику сидов/личей с Rutracker
    log::debug!("--- Обновление статистики (сиды/личи) с Rutracker ---");
    let problematic_ids = get_api_peer_stats_by_hash_async(&mut my_torrents).await?;
    log::debug!("✅ Статистика успешно обновлена.");

    // 3. Если есть проблемные (ненайденные по хешу) торренты, разбираемся с ними
    let (updates_count, deletions_count) = if !problematic_ids.is_empty() {
        log::warn!(
            "--- ⚠️ Обнаружены проблемные торренты (не найдены на Rutracker): {} шт. ---",
            problematic_ids.len()
        );
        log::debug!("Запрос хешей для проблемных ID...");

        let hashes_map = get_api_torrent_hash_by_id_async(&problematic_ids).await?;
        log::debug!("Получены хеши для {} ID. Анализ...", hashes_map.len());

        handle_problematic_torrents(
            client,
            &my_torrents,
            &hashes_map,
            client_for_download,
            headers,
            dry_run,
        )
        .await?
    } else {
        // Если проблемных ID не было, то и действий 0
        (0, 0)
    };

    if updates_count > 0 || deletions_count > 0 {
        if !dry_run {
            log::info!(
                "--- 📊 Сводка: Обновлено: {}, Удалено: {} ---",
                updates_count,
                deletions_count
            );
        } else {
            // Если мы в dry_run, но *были бы* действия
            log::info!(
                "--- 📊 Сводка (Dry Run): Было бы обновлено: {}, Было бы удалено: {} ---",
                updates_count,
                deletions_count
            );
        }
    } else {
        // Либо не было проблемных, либо проблемные не привели к действиям
        log::info!("--- 📊 Сводка: Все торренты актуальны. Обновлений не найдено. ---");
    }

    // 4. Выводим итог
    // log_summary(&my_torrents);
    Ok(())
}

/// Шаг 1: Получение и парсинг торрентов из qBittorrent
async fn get_qbit_torrents(client: &Qbit) -> Result<Vec<Torrent>, Box<dyn Error>> {
    let torrents_info = client.get_torrent_list(Default::default()).await?;
    log::debug!("--- Обработка торрентов ({} шт.) ---", torrents_info.len());

    let mut my_torrents: Vec<Torrent> = Vec::new();

    for torrent_info in torrents_info.iter() {
        let name = torrent_info.name.clone().unwrap_or_default();
        let hash = torrent_info.hash.clone().unwrap_or_default();

        if hash.is_empty() {
            log::warn!("⚠️ Торрент '{}' пропущен (отсутствует хеш)!", name);
            continue;
        }

        let tracker = torrent_info.tracker.clone().unwrap_or_default();
        if !tracker.contains("rutracker") {
            log::debug!(
                "Пропущен (не Rutracker): {} (трекер: {})",
                name.chars().take(20).collect::<String>(),
                tracker.chars().take(20).collect::<String>()
            );
            continue; // Пропускаем этот торрент, переходим к следующему
        }

        // Получаем путь сохранения
        let save_path = torrent_info.save_path.clone().unwrap_or_default();

        match client.get_torrent_properties(&hash).await {
            Ok(properties) => {
                let comment = properties.comment.clone().unwrap_or_default();
                let torrent_id = extract_torrent_id_from_comment(&comment);
                let state = torrent_info
                    .state
                    .as_ref()
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_default();
                let category = torrent_info.category.clone().unwrap_or_default();
                let tags = torrent_info.tags.clone().unwrap_or_default();
                let size = torrent_info.size.unwrap_or(0) as u64;

                my_torrents.push(Torrent {
                    name,
                    torrent_hash: hash,
                    torrent_id,
                    tracker,
                    comment,
                    state,
                    category,
                    tags,
                    size,
                    seeders: 0,
                    leechers: 0,
                    save_path,
                });
            }
            Err(e) => {
                log::warn!("⚠️ Не удалось получить свойства для {}: {}", name, e);
            }
        }
    }
    Ok(my_torrents)
}

/// Шаг 3: Цикл по проблемным торрентам для принятия решения
async fn handle_problematic_torrents(
    client: &Qbit,
    my_torrents: &[Torrent],
    hashes_map: &HashMap<String, Option<String>>,
    client_for_download: &Client,
    headers: &HeaderMap,
    dry_run: bool,
) -> Result<(u32, u32), Box<dyn Error>> {
    let mut updates_count = 0;
    let mut deletions_count = 0;

    for torrent in my_torrents.iter().filter(|t| !t.torrent_id.is_empty()) {
        if let Some(hash_option) = hashes_map.get(&torrent.torrent_id) {
            match hash_option {
                // СЛУЧАЙ 1: Торрент НАЙДЕН (хеш есть)
                Some(new_hash) => {
                    if !new_hash.eq_ignore_ascii_case(&torrent.torrent_hash) {
                        // Хеши не совпадают = ОБНОВЛЕНИЕ
                        match handle_update(
                            client,
                            torrent,
                            new_hash,
                            client_for_download,
                            headers,
                            dry_run,
                        )
                        .await
                        {
                            Ok(true) => updates_count += 1,  // Реальное обновление
                            Ok(false) => updates_count += 1, // Считаем dry-run, чтобы показать в сводке
                            Err(e) => {
                                log::error!(
                                    "❌ Ошибка при обновлении торрента {}: {}",
                                    torrent.name,
                                    e
                                )
                            }
                        }
                    } else {
                        log::debug!(
                            "Торрент '{}' (ID: {}) найден по ID, и хеш ({}) совпадает.",
                            torrent.name,
                            torrent.torrent_id,
                            torrent.torrent_hash
                        );
                    }
                }
                // СЛУЧАЙ 2: Торрент НЕ НАЙДЕН (хеш = null) = УДАЛЕНИЕ
                None => {
                    match handle_deletion(client, torrent, dry_run).await {
                        Ok(true) => deletions_count += 1,  // Реальное удаление
                        Ok(false) => deletions_count += 1, // Считаем dry-run, чтобы показать в сводке
                        Err(e) => {
                            log::error!("❌ Ошибка при удалении торрента {}: {}", torrent.name, e)
                        }
                    }
                }
            }
        }
    }
    Ok((updates_count, deletions_count))
}

/// Действие: Обновить торрент
async fn handle_update(
    client: &Qbit,
    torrent: &Torrent,
    new_hash: &str,
    client_for_download: &Client,
    headers: &HeaderMap,
    dry_run: bool,
) -> Result<bool, Box<dyn Error>> {
    log::warn!(
        "🔄 ОБНОВЛЕНИЕ: Торрент '{}' (ID: {}) обновлен на трекере.",
        torrent.name,
        torrent.torrent_id
    );
    log::info!(
        "Старый хеш: {}. Новый хеш: {}",
        torrent.torrent_hash,
        new_hash
    );

    if dry_run {
        log::warn!("🟢 DRY-RUN: Обновление пропущено (включен пробный запуск).");
        return Ok(false);
    }

    // 1. Парсим ID
    let topic_id: u64 = match torrent.torrent_id.parse() {
        Ok(id) => id,
        Err(e) => {
            log::error!(
                "❌ Не удалось спарсить ID торрента '{}' (ID: {}) в u64: {}",
                torrent.name,
                torrent.torrent_id,
                e
            );
            return Err(e.into()); //return Err(Box::new(e));
        }
    };

    // 2. Скачиваем новый .torrent файл
    log::debug!("Скачивание t{}.torrent...", topic_id);
    let torrent_file_path =
        rutracker_api::download_torrent(client_for_download, headers, topic_id).await?;
    log::debug!(
        "Файл {} скачан. Добавление в qBittorrent...",
        torrent_file_path
    );

    // 3. Добавляем новый торрент в qBittorrent
    log::debug!("--- Попытка добавить торрент из ФАЙЛА ---");

    if let Err(e) = add_torrent_from_file(
        client,
        &torrent_file_path,
        &torrent.save_path,
        &torrent.category,
        &torrent.tags,
    )
    .await
    {
        log::error!("❌ Не удалось добавить торрент из файла: {}", e);
        let _ = fs::remove_file(&torrent_file_path).await;
        return Err(e);
    }
    log::info!(
        "✅ Новый торрент ({}) успешно добавлен в qBittorrent (путь: {}).",
        torrent_file_path,
        torrent.save_path
    );

    // 4. Удаляем временный .torrent файл
    fs::remove_file(&torrent_file_path).await?;
    log::debug!("Временный файл {} удален.", torrent_file_path);

    // Шаг 5. Удаляем СТАРЫЙ торрент
    log::info!(
        "Удаление старого торрента (хеш: {})...",
        torrent.torrent_hash
    );
    let hashes_to_delete = vec![torrent.torrent_hash.clone()];
    // НЕ удаляем файлы (false), так как они нужны новому торренту
    client.delete_torrents(hashes_to_delete, false).await?;
    log::info!("✅ Старый торрент успешно удален (файлы сохранены).");

    Ok(true)
}

/// Действие: Удалить торрент
async fn handle_deletion(
    client: &Qbit,
    torrent: &Torrent,
    dry_run: bool,
) -> Result<bool, Box<dyn Error>> {
    log::warn!(
        "❌ УДАЛЕН: Торрент '{}' (ID: {}) удален с трекера.",
        torrent.name,
        torrent.torrent_id
    );
    log::info!(
        "Попытка удаления из qBittorrent (хеш: {})...",
        torrent.torrent_hash
    );

    if dry_run {
        log::warn!("🟢 DRY-RUN: Удаление пропущено (включен пробный запуск).");
        return Ok(false);
    }

    // Удаляем торрент И ЕГО ФАЙЛЫ (true), так как он больше не нужен
    let hashes_to_delete = vec![torrent.torrent_hash.clone()];
    client.delete_torrents(hashes_to_delete, true).await?;
    log::info!("Успешно удален из qBittorrent (включая файлы).");

    Ok(true)
}

/// Шаг 4: Вывод итоговой сводки
fn _log_summary(my_torrents: &[Torrent]) {
    let count = my_torrents.len();
    let start_index = count.saturating_sub(6);

    log::debug!(
        "--- Собранный массив (показаны последние {} из {} шт.) ---",
        std::cmp::min(6, count),
        count
    );

    for torrent in my_torrents.iter().skip(start_index) {
        log::debug!("{:?}", torrent);
    }
}

/// Функция для добавления торрента из .torrent файла
async fn add_torrent_from_file(
    client: &Qbit,
    file_path: &str,
    save_path: &str,
    category: &str,
    tags: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    log::debug!("Чтение файла торрента: {}", file_path);

    // 1. Чтение .torrent файла
    let torrent_content = match fs::read(file_path).await {
        Ok(content) => content,
        Err(e) => {
            log::error!("❌ Ошибка: Не удалось прочитать файл '{}'.", file_path);
            return Err(e.into());
        }
    };

    // 2. Создаем TorrentFile
    let torrent_file = TorrentFile {
        data: torrent_content,
        filename: file_path.to_string(),
    };

    // 3. Создаем TorrentSource
    let torrent_source = TorrentSource::TorrentFiles {
        torrents: vec![torrent_file],
    };

    // 4. Создаем AddTorrentArg (с указанием savepath)
    /*
    let arg = AddTorrentArg::builder()
        .source(torrent_source)
        .savepath(save_path.to_string())
        .build();
    */

    // 4. (ИЗМЕНЕНО) Создаем AddTorrentArg (с указанием savepath, category, tags)
    let arg = AddTorrentArg::builder()
        .source(torrent_source)
        .savepath(save_path.to_string())
        .tags(tags.to_string())
        .category(category.to_string())
        .build();

    /*
    if !category.is_empty() {
        log::debug!("    -> Установка категории: {}", category);
        arg_builder = arg_builder.category(category.to_string());
    }

    // (НОВОЕ) Добавляем теги, если они не пустые
    if !tags.is_empty() {
        log::debug!("    -> Установка тегов: {}", tags);
        arg_builder = arg_builder.tags(tags.to_string());
    }

    let arg = arg_builder.build(); // Собираем аргументы

     */
    // (НОВОЕ) Добавляем категорию, если она не пустая

    // 5. Вызываем client.add_torrent
    log::debug!("Отправка файла торрента в qBittorrent...");
    match client.add_torrent(arg).await {
        Ok(_) => Ok(()),
        Err(e) => {
            log::error!("❌ Ошибка при добавлении торрента из файла: {}", e);
            Err(e.into())
        }
    }
}
