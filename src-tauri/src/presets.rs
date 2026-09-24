//! Вшитые пресеты всех движков. Аргументы хранят плейсхолдеры:
//! `%ENGINE_ROOT%` — корень движка (подставляется при запуске),
//! `%GameFilterTCP%`/`%GameFilterUDP%` — игровой фильтр (profiles::apply_game_filter).

use crate::config::Profile;
use std::path::Path;

const ENGINE_ROOT: &str = "%ENGINE_ROOT%";

/// Идентификатор профиля-пресета по его короткому id. Единая точка правды:
/// и вшитые (PresetDef), и OTA-пресеты (updater.rs) строят id одинаково.
pub fn preset_profile_id(id: &str) -> String {
    format!("preset:{id}")
}

pub struct PresetDef {
    pub id: &'static str,
    pub engine: &'static str,
    pub name: &'static str,
    pub args: &'static [&'static str],
}

impl PresetDef {
    pub fn args_vec(&self) -> Vec<String> {
        self.args.iter().map(|s| s.to_string()).collect()
    }

    /// Пресет как профиль: builtin, источник «preset:» (не редактируется как свой).
    pub fn to_profile(&self) -> Profile {
        preset_profile(self.id, self.engine, self.name, self.args_vec())
    }
}

/// Собирает builtin-профиль пресета из готовых полей. Используется и вшитой
/// таблицей (PresetDef::to_profile), и OTA-пресетами (updater.rs).
pub fn preset_profile(id: &str, engine: &str, name: &str, args: Vec<String>) -> Profile {
    let pid = preset_profile_id(id);
    Profile {
        id: pid.clone(),
        name: name.into(),
        engine: engine.into(),
        args,
        builtin: true,
        source: Some(pid),
        updated_at: None,
    }
}

/// Вшитые пресеты, вырезанные в новых версиях (id без префикса `preset:`).
/// Их профили удаляются при старте — иначе остаются мёртвые стратегии
/// (например `-7/-8/-9` у GoodbyeDPI v0.2.2 → `unknown option`).
/// Чистим ТОЛЬКО этот явный список: пресеты, доставленные по воздуху (OTA),
/// могут иметь id, которых нет во вшитой таблице, и удалять их нельзя.
pub const REMOVED_PRESET_IDS: [&str; 3] = ["goodbyedpi-7", "goodbyedpi-8", "goodbyedpi-9"];

pub fn builtin_presets() -> Vec<PresetDef> {
    vec![
        PresetDef {
            id: "flowseal-general",
            engine: "flowseal",
            name: "Flowseal · General",
            args: &[
                "--wf-tcp=80,443,2053,2083,2087,2096,8443,%GameFilterTCP%",
                "--wf-udp=443,19294-19344,50000-50100,%GameFilterUDP%",
                "--filter-udp=443",
                "--hostlist=%ENGINE_ROOT%lists/list-general.txt",
                "--hostlist=%ENGINE_ROOT%lists/list-general-user.txt",
                "--hostlist-exclude=%ENGINE_ROOT%lists/list-exclude.txt",
                "--hostlist-exclude=%ENGINE_ROOT%lists/list-exclude-user.txt",
                "--dpi-desync=fake",
                "--dpi-desync-repeats=6",
                "--dpi-desync-fake-quic=%ENGINE_ROOT%bin/quic_initial_www_google_com.bin",
                "--new",
                "--filter-tcp=80,443",
                "--hostlist=%ENGINE_ROOT%lists/list-general.txt",
                "--hostlist=%ENGINE_ROOT%lists/list-general-user.txt",
                "--hostlist-exclude=%ENGINE_ROOT%lists/list-exclude.txt",
                "--hostlist-exclude=%ENGINE_ROOT%lists/list-exclude-user.txt",
                "--dpi-desync=multisplit",
                "--dpi-desync-split-pos=1",
                "--dpi-desync-split-seqovl=568",
                "--dpi-desync-split-seqovl-pattern=%ENGINE_ROOT%bin/tls_clienthello_4pda_to.bin",
                "--new",
                "--filter-tcp=443",
                "--hostlist=%ENGINE_ROOT%lists/list-google.txt",
                "--dpi-desync=multisplit",
                "--dpi-desync-split-pos=1",
                "--dpi-desync-split-seqovl=681",
                "--dpi-desync-split-seqovl-pattern=%ENGINE_ROOT%bin/tls_clienthello_www_google_com.bin",
            ],
        },
        // Дословный порт официального preset2_example.cmd из bol-van/zapret-win-bundle
        // (zapret-winws/preset2_example.cmd): HTTP fake/fakedsplit + TLS youtube-hostlist +
        // TLS general + QUIC (hostlist и general) + wireguard/stun/discord.
        // Проверен `winws2 --dry-run`: 6 профилей, hostlist грузится.
        PresetDef {
            id: "zapret2-general",
            engine: "zapret2",
            name: "zapret2 · General (официальный preset2)",
            args: &[
                "--wf-tcp-out=80,443",
                "--lua-init=@%ENGINE_ROOT%lua/zapret-lib.lua",
                "--lua-init=@%ENGINE_ROOT%lua/zapret-antidpi.lua",
                "--lua-init=fake_default_tls = tls_mod(fake_default_tls,'rnd,rndsni')",
                "--blob=quic_google:@%ENGINE_ROOT%files/quic_initial_www_google_com.bin",
                "--wf-raw-part=@%ENGINE_ROOT%windivert.filter/windivert_part.discord_media.txt",
                "--wf-raw-part=@%ENGINE_ROOT%windivert.filter/windivert_part.stun.txt",
                "--wf-raw-part=@%ENGINE_ROOT%windivert.filter/windivert_part.wireguard.txt",
                "--wf-raw-part=@%ENGINE_ROOT%windivert.filter/windivert_part.quic_initial_ietf.txt",
                "--filter-tcp=80",
                "--filter-l7=http",
                "--out-range=-d10",
                "--payload=http_req",
                "--lua-desync=fake:blob=fake_default_http:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5",
                "--lua-desync=fakedsplit:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5",
                "--new",
                "--filter-tcp=443",
                "--filter-l7=tls",
                "--hostlist=%ENGINE_ROOT%files/list-youtube.txt",
                "--out-range=-d10",
                "--payload=tls_client_hello",
                "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=11:tls_mod=rnd,dupsid,sni=www.google.com",
                "--lua-desync=multidisorder:pos=1,midsld",
                "--new",
                "--filter-tcp=443",
                "--filter-l7=tls",
                "--out-range=-d10",
                "--payload=tls_client_hello",
                "--lua-desync=fake:blob=fake_default_tls:tcp_md5:tcp_seq=-10000:repeats=6",
                "--lua-desync=multidisorder:pos=midsld",
                "--new",
                "--filter-udp=443",
                "--filter-l7=quic",
                "--hostlist=%ENGINE_ROOT%files/list-youtube.txt",
                "--payload=quic_initial",
                "--lua-desync=fake:blob=quic_google:repeats=11",
                "--new",
                "--filter-udp=443",
                "--filter-l7=quic",
                "--payload=quic_initial",
                "--lua-desync=fake:blob=fake_default_quic:repeats=11",
                "--new",
                "--filter-l7=wireguard,stun,discord",
                "--payload=wireguard_initiation,wireguard_cookie,stun,discord_ip_discovery",
                "--lua-desync=fake:blob=0x00000000000000000000000000000000:repeats=2",
            ],
        },
        // Узкий YouTube-вариант: только hostlist-блоки TLS+QUIC (быстрее general).
        PresetDef {
            id: "zapret2-youtube",
            engine: "zapret2",
            name: "zapret2 · YouTube (hostlist TLS + QUIC)",
            args: &[
                "--wf-tcp-out=80,443",
                "--lua-init=@%ENGINE_ROOT%lua/zapret-lib.lua",
                "--lua-init=@%ENGINE_ROOT%lua/zapret-antidpi.lua",
                "--lua-init=fake_default_tls = tls_mod(fake_default_tls,'rnd,rndsni')",
                "--blob=quic_google:@%ENGINE_ROOT%files/quic_initial_www_google_com.bin",
                "--wf-raw-part=@%ENGINE_ROOT%windivert.filter/windivert_part.quic_initial_ietf.txt",
                "--filter-tcp=443",
                "--filter-l7=tls",
                "--hostlist=%ENGINE_ROOT%files/list-youtube.txt",
                "--out-range=-d10",
                "--payload=tls_client_hello",
                "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=11:tls_mod=rnd,dupsid,sni=www.google.com",
                "--lua-desync=multidisorder:pos=1,midsld",
                "--new",
                "--filter-udp=443",
                "--filter-l7=quic",
                "--hostlist=%ENGINE_ROOT%files/list-youtube.txt",
                "--payload=quic_initial",
                "--lua-desync=fake:blob=quic_google:repeats=11",
            ],
        },
        // GoodbyeDPI v0.2.2: режимы только -1..-6 (легаси -1..-4, современные -5,-6).
        // Режимов -7/-8/-9 в этой версии НЕТ (exe вернул «unknown option»).
        PresetDef {
            id: "goodbyedpi-5",
            engine: "goodbyedpi",
            name: "GoodbyeDPI · 5 (modern, дефолт)",
            args: &["-5"],
        },
        PresetDef {
            id: "goodbyedpi-6",
            engine: "goodbyedpi",
            name: "GoodbyeDPI · 6 (modern)",
            args: &["-6"],
        },
        PresetDef {
            id: "goodbyedpi-4",
            engine: "goodbyedpi",
            name: "GoodbyeDPI · 4 (legacy)",
            args: &["-4"],
        },
        PresetDef {
            id: "goodbyedpi-3",
            engine: "goodbyedpi",
            name: "GoodbyeDPI · 3 (legacy)",
            args: &["-3"],
        },
        PresetDef {
            id: "goodbyedpi-1",
            engine: "goodbyedpi",
            name: "GoodbyeDPI · 1 (legacy)",
            args: &["-1"],
        },
        // «RU + DNS»: подмена DNS на Яндекс-резолвер (dnsredir). База — modern -5.
        PresetDef {
            id: "goodbyedpi-ru-dns",
            engine: "goodbyedpi",
            name: "GoodbyeDPI · RU + DNS",
            args: &["-5", "--dns-addr", "77.88.8.8", "--dns-port", "1253"],
        },
        PresetDef {
            id: "dpibreak-default",
            engine: "dpibreak",
            name: "DPIBreak · режим 0,1 (дефолт)",
            args: &["-o", "0,1"],
        },
        PresetDef {
            id: "dpibreak-aggressive",
            engine: "dpibreak",
            name: "DPIBreak · 0,5 + fake-autottl",
            args: &["-o", "0,5", "-a"],
        },
        PresetDef {
            id: "dpibreak-autottl",
            engine: "dpibreak",
            name: "DPIBreak · fake-autottl",
            args: &["-a"],
        },
    ]
}

/// Подставляет корень движка вместо `%ENGINE_ROOT%` (с хвостовым разделителем:
/// `%ENGINE_ROOT%lua/...` → `D:\engines\zapret2\lua/...`). Аргументы без
/// плейсхолдера не меняются.
pub fn apply_engine_root(args: &[String], root: &Path) -> Vec<String> {
    let base = if root.as_os_str().is_empty() {
        String::new()
    } else {
        format!("{}{}", root.to_string_lossy(), std::path::MAIN_SEPARATOR)
    };
    args.iter()
        .map(|a| {
            if a.contains(ENGINE_ROOT) {
                // Нормализуем слэши в путях: пресеты пишем с '/', движкам на
                // Windows нужен '\'.
                a.replace(ENGINE_ROOT, &base)
                    .replace('/', std::path::MAIN_SEPARATOR_STR)
            } else {
                a.clone()
            }
        })
        .collect()
}

/// Полная подготовка аргументов профиля к запуску: игровой фильтр, затем корень движка.
pub fn prepare_args(args: &[String], root: &Path, tcp: &str, udp: &str) -> Vec<String> {
    apply_engine_root(&crate::profiles::apply_game_filter(args, tcp, udp), root)
}

/// JSON-набор вшитых пресетов для ассета релиза `presets.json`. Формат читает
/// `updater::parse_preset_set`. Источник правды — эта же таблица, поэтому ассет
/// при релизе генерируется отсюда (см. `emit_preset_set_json` и env
/// ZGUI_PRESETS_OUT в тестах).
#[allow(dead_code)] // генератор ассета релиза: вызов из теста по ZGUI_PRESETS_OUT
pub fn preset_set_json(version: &str) -> String {
    // Собираем через serde_json: ручной эскейп ломался бы на управляющих
    // символах и нестандартных кавычках в именах/аргументах пресетов.
    let presets: Vec<serde_json::Value> = builtin_presets()
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "engine": p.engine,
                "name": p.name,
                "args": p.args,
            })
        })
        .collect();
    serde_json::json!({ "version": version, "presets": presets }).to_string()
}

/// Выгружает актуальный ассет пресетов в указанный путь (для подготовки релиза).
#[allow(dead_code)] // генератор ассета релиза: вызов из теста по ZGUI_PRESETS_OUT
pub fn emit_preset_set_json(path: &Path, version: &str) -> Result<(), String> {
    std::fs::write(path, preset_set_json(version)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_cover_all_engines_and_substitute_root() {
        let ps = builtin_presets();
        assert!(ps.iter().any(|p| p.engine == "flowseal"));
        assert!(ps.iter().any(|p| p.engine == "zapret2" && p.args.iter().any(|a| a.contains("--lua-desync"))));
        assert!(ps.iter().any(|p| p.engine == "goodbyedpi" && p.args.contains(&"-5")));
        assert!(ps.iter().any(|p| p.engine == "dpibreak" && p.args.iter().any(|a| a.starts_with("-o"))));

        // Движки пресетов зарегистрированы, идентификаторы уникальны.
        for p in &ps {
            assert!(crate::config::engine_def(p.engine).is_some(), "незарегистрированный движок: {}", p.engine);
        }
        let ids: std::collections::HashSet<&str> = ps.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), ps.len(), "дубликаты id пресетов");

        let out = apply_engine_root(
            &["--lua-init=@%ENGINE_ROOT%lua/zapret-lib.lua".into(), "plain".into()],
            Path::new(r"D:\e2"),
        );
        assert!(out[0].contains(r"D:\e2\lua\zapret-lib.lua") && !out[0].contains('%'));
        assert_eq!(out[1], "plain"); // без плейсхолдера — не трогаем
    }

    #[test]
    fn generated_set_round_trips_through_parser() {
        // Ассет релиза (preset_set_json) должен читаться тем же парсером, что и
        // OTA-набор — иначе опубликованный presets.json окажется несовместим.
        let json = preset_set_json("2026.09.24");
        let set = crate::updater::parse_preset_set(json.as_bytes()).unwrap();
        assert_eq!(set.version, "2026.09.24");
        assert_eq!(set.presets.len(), builtin_presets().len(), "все пресеты должны пережить round-trip");
        assert!(set.presets.iter().any(|p| p.engine == "zapret2" && p.args.iter().any(|a| a.contains("--lua-desync"))));
    }

    #[test]
    fn emit_asset_when_requested() {
        // Генерация ассета релиза только по запросу (CI/подготовка релиза):
        // ZGUI_PRESETS_OUT=<путь> ZGUI_PRESETS_VERSION=<версия>
        if let Ok(out) = std::env::var("ZGUI_PRESETS_OUT") {
            let ver = std::env::var("ZGUI_PRESETS_VERSION").unwrap_or_else(|_| "0".into());
            emit_preset_set_json(Path::new(&out), &ver).unwrap();
            eprintln!("presets.json выгружен в {out} (версия {ver})");
        }
    }
}
