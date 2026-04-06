// src/lib.rs

//!
//! Модуль для работы с торрентами и API rutracker.cc
//!

pub mod rutracker_api;
pub mod torrent;

use anyhow::{Context, Result};
use qbit_rs::{
    model::{AddTorrentArg, Credential, TorrentFile, TorrentSource},
    Qbit,
};
use rutracker_api::{
    extract_torrent_id_from_comment, get_api_limit_async, get_api_peer_stats_by_hash_async,
    get_api_torrent_hash_by_id_async,
};
use std::collections::HashMap;
use torrent::Torrent;

use reqwest::header::{HeaderMap, HeaderValue, COOKIE, USER_AGENT};
use reqwest::Client;
use serde::Deserialize;
use tokio::fs;

pub fn init_logger() {
    match log4rs::init_file("log4rs.yaml", Default::default()) {
        Ok(_) => log::debug!("log4rs.yaml загружен, логгер инициализирован."),
        Err(e) => log::error!(
            "❌ Ошибка инициализации log4rs (файл log4rs.yaml не найден?): {}",
            e
        ),
    }
}

#[derive(Deserialize, Debug)]
pub struct QbitConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, Debug)]
pub struct RutrackerConfig {
    pub bb_session_cookie: String,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub dry_run: bool,
    pub qbit: QbitConfig,
    pub rutracker: RutrackerConfig,
}

pub async fn run_helper(config: Config) -> Result<()> {
    let client_for_download = Client::builder().build()?;

    let mut headers = HeaderMap::new();
    let cookie_string = format!("bb_session={}", config.rutracker.bb_session_cookie);
    headers.insert(COOKIE, HeaderValue::from_str(&cookie_string)?);
    headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/100.0.0.0 Safari/537.36"));

    log::info!("Подключение к {}...", config.qbit.url);

    let credential = Credential::new(config.qbit.username.clone(), config.qbit.password.clone());
    let client = Qbit::new(config.qbit.url.as_str(), credential);

    if let Err(e) = process_torrents(&client, &client_for_download, &headers, config.dry_run).await
    {
        return Err(e).context("❌ Ошибка при обработке торрентов. Убедитесь, что qBittorrent запущен и учетные данные верны.");
    }

    Ok(())
}

async fn process_torrents(
    client: &Qbit,
    client_for_download: &Client,
    headers: &HeaderMap,
    dry_run: bool,
) -> Result<()> {
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

    // Запрашиваем лимит один раз для всех последующих запросов API!
    let api_limit = match get_api_limit_async().await {
        Ok(lim) => (lim as usize).min(50),
        Err(e) => {
            log::warn!("⚠️ Не удалось получить лимит API, используем 20: {}", e);
            20
        }
    };

    log::debug!("--- Обновление статистики (сиды/личи) с Rutracker ---");
    let problematic_ids = get_api_peer_stats_by_hash_async(&mut my_torrents, api_limit).await?;
    log::debug!("✅ Статистика успешно обновлена.");

    let (updates_count, deletions_count) = if !problematic_ids.is_empty() {
        log::warn!(
            "--- ⚠️ Обнаружены проблемные торренты (не найдены на Rutracker): {} шт. ---",
            problematic_ids.len()
        );
        log::debug!("Запрос хешей для проблемных ID...");

        let hashes_map = get_api_torrent_hash_by_id_async(&problematic_ids, api_limit).await?;
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
            log::info!(
                "--- 📊 Сводка (Dry Run): Было бы обновлено: {}, Было бы удалено: {} ---",
                updates_count,
                deletions_count
            );
        }
    } else {
        log::info!("--- 📊 Сводка: Все торренты актуальны. Обновлений не найдено. ---");
    }

    Ok(())
}

async fn get_qbit_torrents(client: &Qbit) -> Result<Vec<Torrent>> {
    let torrents_info = client.get_torrent_list(Default::default()).await?;
    log::debug!("--- Обработка торрентов ({} шт.) ---", torrents_info.len());

    let mut my_torrents: Vec<Torrent> = Vec::new();

    for torrent_info in torrents_info.iter() {
        let name = torrent_info.name.clone().unwrap_or_default();
        // СРАЗУ приводим хеш к нижнему регистру, чтобы избежать аллокаций при поиске
        let hash = torrent_info.hash.clone().unwrap_or_default().to_lowercase();

        if hash.is_empty() {
            log::warn!("⚠️ Торрент '{}' пропущен (отсутствует хеш)!", name);
            continue;
        }

        let tracker = torrent_info.tracker.clone().unwrap_or_default();
        if !tracker.contains("rutracker") {
            continue;
        }

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

async fn handle_problematic_torrents(
    client: &Qbit,
    my_torrents: &[Torrent],
    hashes_map: &HashMap<String, Option<String>>,
    client_for_download: &Client,
    headers: &HeaderMap,
    dry_run: bool,
) -> Result<(u32, u32)> {
    let mut updates_count = 0;
    let mut deletions_count = 0;

    for torrent in my_torrents.iter().filter(|t| !t.torrent_id.is_empty()) {
        if let Some(hash_option) = hashes_map.get(&torrent.torrent_id) {
            match hash_option {
                Some(new_hash) => {
                    if !new_hash.eq_ignore_ascii_case(&torrent.torrent_hash) {
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
                            Ok(_) => updates_count += 1,
                            Err(e) => log::error!(
                                "❌ Ошибка при обновлении торрента {}: {}",
                                torrent.name,
                                e
                            ),
                        }
                    }
                }
                None => match handle_deletion(client, torrent, dry_run).await {
                    Ok(_) => deletions_count += 1,
                    Err(e) => {
                        log::error!("❌ Ошибка при удалении торрента {}: {}", torrent.name, e)
                    }
                },
            }
        }
    }
    Ok((updates_count, deletions_count))
}

async fn handle_update(
    client: &Qbit,
    torrent: &Torrent,
    new_hash: &str,
    client_for_download: &Client,
    headers: &HeaderMap,
    dry_run: bool,
) -> Result<bool> {
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
        return Ok(false);
    }

    let topic_id: u64 = torrent
        .torrent_id
        .parse()
        .with_context(|| format!("❌ Не удалось спарсить ID торрента '{}'", torrent.name))?;

    let torrent_file_path =
        rutracker_api::download_torrent(client_for_download, headers, topic_id).await?;

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

    fs::remove_file(&torrent_file_path).await?;

    let hashes_to_delete = vec![torrent.torrent_hash.clone()];
    client.delete_torrents(hashes_to_delete, false).await?;

    Ok(true)
}

async fn handle_deletion(client: &Qbit, torrent: &Torrent, dry_run: bool) -> Result<bool> {
    log::warn!(
        "❌ УДАЛЕН: Торрент '{}' (ID: {}) удален с трекера.",
        torrent.name,
        torrent.torrent_id
    );

    if dry_run {
        return Ok(false);
    }

    let hashes_to_delete = vec![torrent.torrent_hash.clone()];
    client.delete_torrents(hashes_to_delete, true).await?;
    Ok(true)
}

async fn add_torrent_from_file(
    client: &Qbit,
    file_path: &str,
    save_path: &str,
    category: &str,
    tags: &str,
) -> Result<()> {
    let torrent_content = fs::read(file_path)
        .await
        .with_context(|| format!("❌ Ошибка: Не удалось прочитать файл '{}'", file_path))?;

    let torrent_file = TorrentFile {
        data: torrent_content,
        filename: file_path.to_string(),
    };

    let torrent_source = TorrentSource::TorrentFiles {
        torrents: vec![torrent_file],
    };

    let arg = AddTorrentArg::builder()
        .source(torrent_source)
        .savepath(save_path.to_string())
        .tags(tags.to_string())
        .category(category.to_string())
        .build();

    client
        .add_torrent(arg)
        .await
        .with_context(|| "❌ Ошибка при добавлении торрента из файла в qBittorrent")?;

    Ok(())
}
