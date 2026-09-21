use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

const FLOWSEAL_SNAPSHOT: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/flowseal-main.zip"));

const FLOWSEAL_ENGINE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/engine-flowseal.zip"));

pub const SNAPSHOT_INFO: &str = "Official Flowseal main snapshot bundled at build time";
/// Версия вшитого движка Flowseal (входит в состав exe).
pub const ENGINE_VERSION: &str = "1.10.2";
pub const ENGINE_INFO: &str = "Flowseal 1.10.2 release archive bundled in the executable";

/// Распаковывает встроенный релиз движка Flowseal в `<data>/engines/flowseal`,
/// если там ещё нет exe. Возвращает путь к корню движка, если он готов.
pub fn ensure_embedded_engine(data: &Path) -> Result<Option<PathBuf>, String> {
    let root = data.join("engines").join(crate::config::ENGINE_FLOWSEAL);
    if let Some(existing) = engine_root_for(&root) {
        return Ok(Some(existing));
    }
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    extract_all(FLOWSEAL_ENGINE, &root)?;
    let found = match engine_root_for(&root) {
        Some(found) => found,
        None => return Err("во встроенном архиве flowseal не найден winws.exe".into()),
    };
    neutralize_author_autoupdate(&found);
    Ok(Some(found))
}

/// Отключает авторскую авто-проверку обновлений Flowseal: `utils/check_updates.enabled`
/// заставляет каждый *.bat вызывать `service.bat check_updates`, который открывает
/// страницу релиза в браузере. Нам это не нужно (обновления ведёт наш GUI).
pub fn neutralize_author_autoupdate(root: &Path) {
    let flag = root.join("utils").join("check_updates.enabled");
    if flag.exists() {
        let _ = fs::rename(&flag, root.join("utils").join("check_updates.enabled.zgui_disabled"));
        if flag.exists() {
            let _ = fs::remove_file(&flag);
        }
    }
}

/// Публичная обёртка для нормализации корня уже распакованного движка.
pub fn engine_root_for_public(dir: &Path) -> Option<PathBuf> {
    engine_root_for(dir)
}

/// Находит реальный корень движка внутри распакованной папки Flowseal —
/// каталог с `bin/winws.exe`.
fn engine_root_for(dir: &Path) -> Option<PathBuf> {
    let rel = crate::config::find_exe(dir, "winws.exe")?;
    let exe_path = dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    // Поднимаемся от exe вверх до каталога, содержащего marker (bin).
    let mut cur = exe_path.parent().map(|p| p.to_path_buf())?;
    loop {
        if cur.join("bin").is_dir() {
            return Some(cur);
        }
        match cur.parent() {
            Some(p) if p.starts_with(dir) => cur = p.to_path_buf(),
            _ => break,
        }
    }
    // Fallback — сам каталог.
    Some(dir.to_path_buf())
}

/// Полная распаковка архива с обрезкой общего верхнего каталога (безопасно от zip-slip).
fn extract_all(bytes: &[u8], dest: &Path) -> Result<usize, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;

    let mut top: Option<String> = None;
    let mut uniform = true;
    for i in 0..archive.len() {
        let name = archive.by_index(i).map_err(|e| e.to_string())?.name().to_string();
        let trimmed = name.trim_end_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.split_once('/') {
            // Запись внутри каталога: проверяем общий верхний каталог.
            Some((first, _)) => match &top {
                None => top = Some(first.to_string()),
                Some(t) if t == first => {}
                Some(_) => {
                    uniform = false;
                    break;
                }
            },
            // Запись самого каталога-обёртки нередко идёт до его содержимого.
            // Не считаем её отдельным верхним каталогом.
            None => {
                if let Some(top) = top.as_deref() {
                    if top != trimmed {
                        uniform = false;
                    }
                }
            }
        }
    }
    let strip = if uniform { top } else { None };

    let mut written = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let is_dir = entry.is_dir();
        let name = entry.name().to_string();
        let mut relative = name.clone();
        if let Some(t) = &strip {
            relative = relative
                .strip_prefix(&format!("{}/", t))
                .map(|s| s.to_string())
                .unwrap_or(relative);
        }
        if relative.trim_end_matches('/').is_empty() {
            continue;
        }
        let Some(rel) = safe_relative(relative.trim_end_matches('/')) else { continue };
        let out = dest.join(&rel);
        if !out.starts_with(dest) {
            continue;
        }
        if is_dir {
            let _ = fs::create_dir_all(&out);
            continue;
        }
        if let Some(parent) = out.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut content = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut content).map_err(|e| e.to_string())?;
        crate::config::atomic_write(&out, &content)?;
        written += 1;
    }
    Ok(written)
}

pub fn seed_catalog(data: &Path) -> Result<usize, String> {
    let mut written = 0;
    // Remove the legacy system-hosts catalog entry from older portable data.
    let legacy_hosts = data.join("catalog/flowseal/.service/hosts");
    if legacy_hosts.exists() {
        let _ = fs::remove_file(legacy_hosts);
    }
    written += extract_missing(FLOWSEAL_SNAPSHOT, data, flowseal_destination)?;

    let info = data.join("catalog/source-info.txt");
    if !info.exists() {
        crate::config::atomic_write(&info, SNAPSHOT_INFO.as_bytes())?;
        written += 1;
    }
    Ok(written)
}

fn extract_missing(
    bytes: &[u8],
    data: &Path,
    destination: fn(&str) -> Option<PathBuf>,
) -> Result<usize, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut written = 0;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let Some(relative) = name.split_once('/').map(|(_, tail)| tail) else {
            continue;
        };
        let Some(destination) = destination(relative) else {
            continue;
        };
        let destination = data.join(destination);
        if destination.exists() {
            continue;
        }

        let mut content = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut content).map_err(|e| e.to_string())?;
        crate::config::atomic_write(&destination, &content)?;
        written += 1;
    }
    Ok(written)
}

fn flowseal_destination(relative: &str) -> Option<PathBuf> {
    match relative {
        "LICENSE.txt" => return Some(PathBuf::from("catalog/sources/flowseal/LICENSE.txt")),
        "README.md" => return Some(PathBuf::from("catalog/sources/flowseal/README.md")),
        "service.bat" => return Some(PathBuf::from("catalog/sources/flowseal/service.bat")),
        _ => {}
    }

    if let Some(name) = relative.strip_prefix(".service/") {
        if matches!(name, "version.txt" | "ipset-service.txt") {
            return safe_relative(name).map(|p| PathBuf::from("catalog/flowseal/.service").join(p));
        }
    }
    if let Some(name) = relative.strip_prefix("lists/") {
        if matches!(
            name,
            "list-general.txt" | "list-google.txt" | "list-exclude.txt" | "ipset-exclude.txt" | "ipset-all.txt"
        ) {
            return safe_relative(name).map(|p| PathBuf::from("catalog/flowseal/lists").join(p));
        }
    }
    if !relative.contains('/') && relative.to_ascii_lowercase().ends_with(".bat") && !relative.eq_ignore_ascii_case("service.bat") {
        return safe_relative(relative).map(|p| PathBuf::from("catalog/flowseal/raw").join(p));
    }
    None
}

fn safe_relative(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains('\\') || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

pub fn copy_missing(source: &Path, destination: &Path) {
    if source.is_file() && !destination.exists() {
        if let Some(parent) = destination.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(source, destination);
    }
}

pub fn copy_tree_missing(source: &Path, destination: &Path) {
    let Ok(entries) = fs::read_dir(source) else {
        return;
    };
    for entry in entries.flatten() {
        let src = entry.path();
        let dst = destination.join(entry.file_name());
        if src.is_dir() {
            copy_tree_missing(&src, &dst);
        } else {
            copy_missing(&src, &dst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_the_official_config_catalog() {
        let data = std::env::temp_dir().join(format!(
            "zgui-embedded-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&data);
        fs::create_dir_all(&data).unwrap();

        let written = seed_catalog(&data).unwrap();

        assert!(written > 0);
        assert!(data.join("catalog/flowseal/raw/general.bat").is_file());
        assert!(data.join("catalog/flowseal/lists/list-general.txt").is_file());
        assert!(!data.join("catalog/flowseal/.service/hosts").exists());

        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn embedded_engines_contain_exe() {
        let data = std::env::temp_dir().join(format!("zgui-engine-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        fs::create_dir_all(&data).unwrap();

        let fs_root = ensure_embedded_engine(&data).unwrap();
        assert!(fs_root.is_some(), "встроенный flowseal не распаковался");
        let fs_root = fs_root.unwrap();
        assert!(crate::config::find_exe(&fs_root, "winws.exe").is_some());

        // Корень должен содержать bin/, иначе лаунчер соберёт неверные пути
        // (регрессия «process exited immediately»).
        assert!(fs_root.join("bin").is_dir(), "flowseal root без bin/: {:?}", fs_root);

        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn disables_author_autoupdate_flag() {
        let data = std::env::temp_dir().join(format!("zgui-au-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        let utils = data.join("utils");
        fs::create_dir_all(&utils).unwrap();
        let flag = utils.join("check_updates.enabled");
        fs::write(&flag, b"1").unwrap();

        neutralize_author_autoupdate(&data);

        assert!(!flag.exists(), "флаг автопроверки должен быть отключён");
        assert!(utils.join("check_updates.enabled.zgui_disabled").exists());
        let _ = fs::remove_dir_all(&data);
    }
}
