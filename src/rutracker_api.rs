// src/rutracker_api.rs

use crate::torrent;
use anyhow::{Context, Result};
use reqwest::header::{self, HeaderMap};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use tokio::io::AsyncWriteExt;

// --- КОНСТАНТЫ API ---
const API_LIMIT_URL: &str = "https://api.rutracker.cc/v1/get_limit";
const API_TOR_TOPIC_DATA_URL: &str = "https://api.rutracker.cc/v1/get_tor_topic_data";

// --- СТРУКТУРЫ ДЛЯ get_limit ---
#[derive(Deserialize)]
struct ApiResponseLimit {
    result: ResultDataLimit,
}

#[derive(Deserialize)]
struct ResultDataLimit {
    limit: u32,
}

pub async fn get_api_limit_async() -> Result<u32> {
    let response_data: ApiResponseLimit = reqwest::get(API_LIMIT_URL)
        .await
        .context("Ошибка при выполнении запроса к API лимитов")?
        .json::<ApiResponseLimit>()
        .await
        .context("Ошибка при десериализации ответа лимитов")?;

    Ok(response_data.result.limit)
}

pub fn extract_torrent_id_from_comment(comment: &str) -> String {
    comment
        .rsplit_once("t=")
        .map(|(_, id)| id.to_string())
        .unwrap_or_default()
}

#[derive(Deserialize, Debug)]
struct TopicData {
    info_hash: String,
    seeders: u32,
}

#[derive(Deserialize, Debug)]
struct ApiResponseTopicData {
    result: std::collections::HashMap<String, Option<TopicData>>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ApiResponse {
    Success(ApiResponseTopicData),
    Error(ApiErrorResponse),
}

#[derive(Deserialize, Debug)]
struct ApiErrorDetail {
    #[allow(dead_code)]
    code: i32,
    text: String,
}

#[derive(Deserialize, Debug)]
struct ApiErrorResponse {
    error: ApiErrorDetail,
}

fn extract_invalid_hash(error_text: &str) -> Option<&str> {
    error_text
        .rsplit_once("Invalid hash: ")
        .map(|(_, hash)| hash.trim())
}

// --- ОСНОВНАЯ ФУНКЦИЯ API (get_peer_stats) ---
pub async fn get_api_peer_stats_by_hash_async(
    my_torrents: &mut [torrent::Torrent],
    limit: usize,
) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    const MAX_ATTEMPTS_PER_CHUNK: u8 = 5;
    let mut problematic_ids: Vec<String> = Vec::new();

    log::debug!("Обновление статистики порциями по {} торрентов...", limit);

    for torrent_chunk in my_torrents.chunks_mut(limit) {
        let mut current_work_list: Vec<&mut torrent::Torrent> = torrent_chunk.iter_mut().collect();
        let mut attempts = 0;

        'retry_loop: while !current_work_list.is_empty() && attempts < MAX_ATTEMPTS_PER_CHUNK {
            attempts += 1;

            let hashes: Vec<&str> = current_work_list
                .iter()
                .map(|t| t.torrent_hash.as_str())
                .collect();

            let hash_string = hashes.join(",");
            let url = format!("{}?by=hash&val={}", API_TOR_TOPIC_DATA_URL, hash_string);

            match client.get(&url).send().await {
                Ok(response) => {
                    match response.json::<ApiResponse>().await {
                        Ok(ApiResponse::Success(response_data)) => {
                            let mut hash_to_stats: HashMap<String, TopicData> = HashMap::new();
                            for (_, maybe_data) in response_data.result {
                                if let Some(data) = maybe_data {
                                    // Приводим хэш из API к нижнему регистру один раз
                                    hash_to_stats.insert(data.info_hash.to_lowercase(), data);
                                }
                            }

                            for torrent in current_work_list.iter_mut() {
                                // Ищем без аллокаций, т.к. torrent.torrent_hash уже в нижнем регистре
                                if let Some(stats) = hash_to_stats.get(&torrent.torrent_hash) {
                                    torrent.seeders = stats.seeders;
                                    torrent.leechers = 0;
                                } else {
                                    log::warn!(
                                        "⚠️ Хэш не найден на Rutracker: {} (Торрент: {})",
                                        torrent.torrent_hash,
                                        torrent.name
                                    );
                                    if !torrent.torrent_id.is_empty() {
                                        problematic_ids.push(torrent.torrent_id.clone());
                                    }
                                }
                            }
                            break 'retry_loop;
                        }
                        Ok(ApiResponse::Error(error_data)) => {
                            log::warn!("⚠️ Ошибка API Rutracker: {}", error_data.error.text);
                            if let Some(bad_hash) = extract_invalid_hash(&error_data.error.text) {
                                if let Some(index) = current_work_list
                                    .iter()
                                    .position(|t| t.torrent_hash.eq_ignore_ascii_case(bad_hash))
                                {
                                    let removed_torrent = current_work_list.remove(index);
                                    if !removed_torrent.torrent_id.is_empty() {
                                        problematic_ids.push(removed_torrent.torrent_id.clone());
                                    }
                                } else {
                                    break 'retry_loop;
                                }
                            } else {
                                break 'retry_loop;
                            }
                        }
                        Err(e) => {
                            log::error!("❌ Ошибка десериализации JSON: {}", e);
                            break 'retry_loop;
                        }
                    }
                }
                Err(e) => {
                    log::error!("❌ Ошибка HTTP запроса: {}", e);
                    break 'retry_loop;
                }
            }
        }
    }

    Ok(problematic_ids)
}

pub async fn get_api_torrent_hash_by_id_async<T>(
    ids: &[T],
    limit: usize,
) -> Result<HashMap<String, Option<String>>>
where
    T: fmt::Display,
{
    let client = reqwest::Client::new();
    let mut all_results: HashMap<String, Option<String>> = HashMap::new();

    if ids.is_empty() {
        return Ok(all_results);
    }

    for id_chunk in ids.chunks(limit) {
        let id_string = id_chunk
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<String>>()
            .join(",");

        let url = format!("{}?by=topic_id&val={}", API_TOR_TOPIC_DATA_URL, id_string);

        match client.get(&url).send().await {
            Ok(response) => match response.json::<ApiResponseTopicData>().await {
                Ok(response_data) => {
                    for (id, maybe_data) in response_data.result {
                        all_results.insert(id, maybe_data.map(|d| d.info_hash));
                    }
                }
                Err(e) => log::error!("❌ Ошибка десериализации: {}", e),
            },
            Err(e) => log::error!("❌ Ошибка HTTP: {}", e),
        }
    }

    Ok(all_results)
}

pub async fn download_torrent(
    client: &Client,
    headers: &HeaderMap,
    topic_id: u64,
) -> Result<String> {
    let download_url = format!("https://rutracker.org/forum/dl.php?t={}", topic_id);
    let output_filename = format!("t{}.torrent", topic_id);

    log::info!("Попытка скачивания файла (тема {})...", topic_id);

    let mut download_response = client
        .get(&download_url)
        .headers(headers.clone())
        .send()
        .await?;

    let content_type = download_response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or(""))
        .unwrap_or("");

    if download_response.status().is_success() && content_type.contains("application/x-bittorrent")
    {
        let mut output_file = tokio::fs::File::create(&output_filename).await?;

        while let Some(chunk) = download_response.chunk().await? {
            output_file.write_all(&chunk).await?;
        }

        Ok(output_filename)
    } else {
        log::error!("❌ Ошибка авторизации или скачивания.");
        Err(anyhow::anyhow!(
            "Ошибка скачивания: сервер не вернул .torrent файл (статус: {}).",
            download_response.status()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_torrent_id_from_comment() {
        assert_eq!(
            extract_torrent_id_from_comment("https://rutracker.org/forum/viewtopic.php?t=1234567"),
            "1234567"
        );
        assert_eq!(
            extract_torrent_id_from_comment("Просто текст t=98765"),
            "98765"
        );
        assert_eq!(extract_torrent_id_from_comment("Без идентификатора"), "");
    }

    #[test]
    fn test_extract_invalid_hash() {
        assert_eq!(
            extract_invalid_hash("Invalid hash: ABCDEF1234567890"),
            Some("ABCDEF1234567890")
        );
        assert_eq!(extract_invalid_hash("Some other random api error"), None);
    }
}
