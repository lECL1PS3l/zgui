//! Автоподбор стратегии: матрица кандидатов на движок (вшитые пресеты +
//! сгенерированные варианты). Прогон идёт существующим тестером (tester.rs),
//! удачные профили сохраняются с тегом `auto:<движок>`.

pub struct Candidate {
    pub id: String,
    pub name: String,
    pub args: Vec<String>,
}

/// Кандидаты для движка: вшитые пресеты + сгенерированные варианты.
/// Возвращает пустой список для неизвестного движка.
pub fn candidates(engine: &str) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();

    // 1) Вшитые пресеты движка (готовые проверенные наборы).
    for p in crate::presets::builtin_presets()
        .iter()
        .filter(|p| p.engine == engine)
    {
        out.push(Candidate {
            id: p.id.to_string(),
            name: p.name.to_string(),
            args: p.args_vec(),
        });
    }

    // 2) Сгенерированные варианты (короткие, документированные аргументы).
    match engine {
        "goodbyedpi" => {
            for ttl in [4u32, 5, 6, 7, 8] {
                out.push(Candidate {
                    id: format!("gd-ttl{ttl}"),
                    name: format!("GoodbyeDPI · -9 + TTL {ttl}"),
                    args: vec!["-9".into(), "--set-ttl".into(), ttl.to_string()],
                });
            }
            out.push(Candidate {
                id: "gd-chksum".into(),
                name: "GoodbyeDPI · -9 + wrong-chksum".into(),
                args: vec!["-9".into(), "--wrong-chksum".into()],
            });
            out.push(Candidate {
                id: "gd-seq".into(),
                name: "GoodbyeDPI · -9 + wrong-seq".into(),
                args: vec!["-9".into(), "--wrong-seq".into()],
            });
        }
        "dpibreak" => {
            for o in ["0,1", "0,5", "5,0"] {
                for a in [false, true] {
                    let mut args = vec!["-o".to_string(), o.to_string()];
                    let mut name = format!("DPIBreak · -o {o}");
                    if a {
                        args.push("-a".into());
                        name.push_str(" + autottl");
                    }
                    out.push(Candidate {
                        id: format!("dpi-{}-{}", o.replace(',', "_"), if a { "a" } else { "n" }),
                        name,
                        args,
                    });
                }
            }
            for ttl in [4u32, 5, 6, 7, 8] {
                out.push(Candidate {
                    id: format!("dpi-ttl{ttl}"),
                    name: format!("DPIBreak · fake-ttl {ttl}"),
                    args: vec!["--fake-ttl".into(), ttl.to_string()],
                });
            }
        }
        "zapret2" => {
            // База — general-пресет; варьируем TLS-блок (443) заменой lua-desync.
            if let Some(base) = crate::presets::builtin_presets()
                .iter()
                .find(|p| p.id == "zapret2-general")
            {
                let args: Vec<String> = base.args_vec();
                for (id, name, tls) in zapret2_tls_variants() {
                    out.push(Candidate {
                        id: id.into(),
                        name: name.into(),
                        args: zapret2_with_tls(&args, &tls),
                    });
                }
            }
        }
        _ => {}
    }

    // Дедуп по аргументам (разные id, одинаковый набор — не нужен).
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    out.retain(|c| seen.insert(c.args.join("\u{1}")));
    out
}

/// Варианты TLS-блока для zapret2: (id, имя, [lua-desync ...]).
fn zapret2_tls_variants() -> Vec<(&'static str, &'static str, Vec<&'static str>)> {
    vec![
        (
            "z2-fake-auto",
            "zapret2 · fake + autottl",
            vec!["--lua-desync=fake:blob=fake_default_tls:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5:repeats=6"],
        ),
        (
            "z2-fakedsplit",
            "zapret2 · fakedsplit + autottl",
            vec!["--lua-desync=fakedsplit:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5"],
        ),
        (
            "z2-multidisorder",
            "zapret2 · multidisorder",
            vec!["--lua-desync=multidisorder:pos=midsld"],
        ),
    ]
}

/// Заменяет в базовых аргументах zapret2 блок TLS (строки `--lua-desync=...`,
/// идущие после `--filter-l7=tls`) на переданные варианты. Остальное сохраняется.
fn zapret2_with_tls(base: &[String], tls: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_tls = false;
    for a in base {
        if a == "--filter-tcp=443" {
            in_tls = false;
        }
        if a == "--filter-l7=tls" {
            in_tls = true;
            out.push(a.clone());
            continue;
        }
        // Внутри TLS-блока lua-desync заменяем на вариант.
        if in_tls && a.starts_with("--lua-desync=") {
            continue;
        }
        out.push(a.clone());
        // После TLS-блока ставим наш вариант один раз (перед --new).
        if in_tls && a == "--payload=tls_client_hello" {
            for t in tls {
                out.push((*t).to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_cover_generated_engines() {
        let gd = candidates("goodbyedpi");
        assert!(gd.iter().any(|c| c.args.contains(&"--set-ttl".to_string())));
        assert!(gd.iter().any(|c| c.args.contains(&"--wrong-chksum".to_string())));
        let dpi = candidates("dpibreak");
        assert!(dpi.iter().any(|c| c.args.contains(&"--fake-ttl".to_string())));
        assert!(dpi.iter().any(|c| c.args.contains(&"-a".to_string())));
        let z2 = candidates("zapret2");
        assert!(z2.iter().any(|c| c.id == "z2-fakedsplit"));
        // Дедуп: нет двух кандидатов с одинаковыми аргументами.
        let keys: std::collections::HashSet<String> = z2.iter().map(|c| c.args.join("|")).collect();
        assert_eq!(keys.len(), z2.len());
        // Неизвестный движок — пусто.
        assert!(candidates("nope").is_empty());
    }

    #[test]
    fn zapret2_variant_keeps_base_and_swaps_tls() {
        let base = crate::presets::builtin_presets()
            .iter()
            .find(|p| p.id == "zapret2-general")
            .unwrap()
            .args_vec();
        let mut tls = vec!["--lua-desync=multidisorder:pos=midsld".to_string()];
        tls.push("--lua-desync=fake:blob=fake_default_tls".to_string());
        let out = zapret2_with_tls(&base, &["--lua-desync=multidisorder:pos=1,midsld"]);
        // База (wf, lua-init, http-блок, QUIC) сохранена.
        assert!(out.iter().any(|a| a == "--wf-tcp-out=80,443"));
        assert!(out.iter().any(|a| a.starts_with("--lua-init=@")));
        assert!(out.iter().any(|a| a == "--filter-l7=quic"));
        // Старый TLS fake/… заменён на единственный вариант.
        assert!(out.iter().any(|a| a == "--lua-desync=multidisorder:pos=1,midsld"));
        assert!(!out.iter().any(|a| a.contains("repeats=6")));
        let _ = tls;
    }
}
