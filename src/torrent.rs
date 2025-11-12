// src/torrent.rs
// Структура для представления торрента
#[derive(Clone, PartialEq, Eq)]
pub struct Torrent {
    pub name: String,
    pub torrent_hash: String,
    pub torrent_id: String,
    pub tracker: String,
    pub comment: String,
    pub state: String,
    pub category: String,
    pub tags: String, // <-- ДОБАВЛЕНО
    pub size: u64,
    pub seeders: u32,
    pub leechers: u32,
    pub save_path: String,
}
use std::fmt;

// Реализация Debug для компактного однострочного вывода
impl fmt::Debug for Torrent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Рассчитаем компактные поля
        let peers = format!("{}/{}", self.seeders, self.leechers);
        let size_mb = self.size / (1024 * 1024);

        // Используем `debug_struct` для создания строки
        // (ИЗМЕНЕНО) Собираем в `mut debug_struct`, чтобы добавить теги опционально
        let mut debug_struct = f.debug_struct("Torrent");

        debug_struct
            .field("name", &self.name)
            .field("id", &self.torrent_id)
            .field("hash", &self.torrent_hash)
            .field("state", &self.state)
            .field("peers", &peers)
            .field("size_mb", &size_mb)
            .field("category", &self.category);

        // (ИЗМЕНЕНО) Добавляем теги, только если они не пустые
        if !self.tags.is_empty() {
            debug_struct.field("tags", &self.tags);
        }

        debug_struct
            .field("tracker", &self.tracker)
            .field("comment", &self.comment)
            .field("save_path", &self.save_path)
            .finish()
    }
}

// Реализация Display для удобного форматированного вывода информации о торренте
impl fmt::Display for Torrent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Пишем основную информацию
        write!(
            f,
            "Имя: {}\n  - ID: {}\n  - Хеш: {}\n  - Статус: {}\n  - Категория: {}\n  - Размер: {} MB\n  - Сиды: {} | Личи: {}",
            self.name,
            self.torrent_id,
            self.torrent_hash,
            self.state,
            self.category,
            self.size / (1024 * 1024), // Отобразим в МБ
            self.seeders,
            self.leechers
        )?;

        // (ИЗМЕНЕНО) Добавляем теги, если они есть
        if !self.tags.is_empty() {
            write!(f, "\n  - Теги: {}", self.tags)?;
        }

        // Добавляем трекер
        if !self.tracker.is_empty() {
            write!(f, "\n  - Трекер: {}", self.tracker)?;
        }

        // Добавляем путь сохранения, если он есть
        if !self.save_path.is_empty() {
            write!(f, "\n  - Путь: {}", self.save_path)?;
        }

        // Добавляем комментарий, если он есть
        if !self.comment.is_empty() {
            write!(f, "\n  - Комментарий: {}", self.comment)?;
        }

        // `println!` сам добавит перевод строки в конце
        Ok(())
    }
}
