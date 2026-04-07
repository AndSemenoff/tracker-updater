// src/main.rs

use anyhow::{Context, Result};
use clap::Parser;
use config::{Config as ConfigBuilder, File};
use tracker_updater::{run_helper, Config};

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Путь к файлу конфигурации
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

/// Главная асинхронная функция, обрабатывающая ошибки
async fn run() -> Result<()> {
    let args = Args::parse();

    // 1. Инициализация логгера
    tracker_updater::init_logger();

    log::info!("⚙️ Загрузка конфигурации из {}...", args.config);

    // 2. Сборка конфигурации из файла
    let config: Config = ConfigBuilder::builder()
        .add_source(File::with_name(&args.config).required(true))
        .build()
        .with_context(|| {
            format!(
                "❌ Ошибка загрузки файла {}. Убедитесь, что файл существует.",
                args.config
            )
        })?
        .try_deserialize::<Config>()
        .context(
            "❌ Ошибка парсинга конфигурации. Убедитесь, что файл имеет правильную структуру.",
        )?;

    log::debug!(
        "Конфигурация загружена: dry_run = {}, qbit.url = {}",
        config.dry_run,
        config.qbit.url
    );

    if config.dry_run {
        log::warn!("--- 🟢 Включен режим пробного запуска (Dry Run) ---");
        log::warn!("--- 🟢 Никакие торренты не будут изменены или удалены ---");
    }

    // 3. Запуск основного процесса
    run_helper(config).await?;

    log::info!("✅ Работа успешно завершена.");
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        log::error!("❌ Критическая ошибка:\n {:?}", e);
        std::process::exit(1);
    }
}
