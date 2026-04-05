// src/main.rs

use anyhow::{Context, Result};
use config::{Config as ConfigBuilder, File};
use tracker_updater::{run_helper, Config};

/// Главная асинхронная функция, обрабатывающая ошибки
/// Теперь она отвечает только за настройку и запуск
async fn run() -> Result<()> {
    // 1. Инициализация логгера (из нашей библиотеки)
    tracker_updater::init_logger();

    // 2. Сборка конфигурации из файла
    log::info!("⚙️ Загрузка конфигурации из config.toml...");

    let builder = ConfigBuilder::builder()
        // 1. Добавляем значения по умолчанию (если нужно)
        // .add_source(ConfigBuilder::try_from(&Config::default())?)
        // 2. Загружаем из файла `config.toml`. Он обязателен.
        .add_source(File::with_name("config.toml").required(true));

    // 3. (Опционально) Позволяем переопределить из .env или переменных окружения
    // Например, можно будет задать QBIT_URL в окружении, и он заменит
    // значение из файла. Префикс "APP" (можно любой)
    // .add_source(Environment::with_prefix("APP").separator("__"))

    let config: Config = builder.build()
        .context("❌ Ошибка загрузки файла config.toml. Убедитесь, что файл существует в корневой папке.")?
        .try_deserialize::<Config>()
        .context("❌ Ошибка парсинга конфигурации. Убедитесь, что config.toml имеет правильную структуру.")?;

    // Выводим в лог часть конфига (пароль и сессию не выводим!)
    log::debug!(
        "Конфигурация загружена: dry_run = {}, qbit.url = {}",
        config.dry_run,
        config.qbit.url
    );
    if config.dry_run {
        log::warn!("--- 🟢 Включен режим пробного запуска (Dry Run) ---");
        log::warn!("--- 🟢 Никакие торренты не будут изменены или удалены ---");
    }

    // 4. Запуск основного процесса из библиотеки
    run_helper(config).await?;

    log::info!("✅ Работа успешно завершена.");
    Ok(())
}

// Точка входа
fn main() {
    // Эта часть остается без изменений
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) = run().await {
            log::error!("❌ Критическая ошибка:\n {:?}", e);
            std::process::exit(1);
        }
    });
}
