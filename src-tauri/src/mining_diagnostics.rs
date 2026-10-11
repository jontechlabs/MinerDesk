use super::LogLine;

pub fn validate_gpu_selection(profile: &super::MinerProfile, french: bool) -> Result<(),String> {
    let selected=super::gpu_tokens(&profile.gpu_ids);
    if selected.is_empty() { return Ok(()); }
    type Getter = fn(&super::MinerProfile,&str)->Option<i64>;
    let (engine,getters): (&str,Vec<(&str,Getter)>) = if profile.engine == "npminer" {
        ("NPMiner",vec![
            ("--cuda-lock-core-clocks",|p,id|super::effective_core_clock(p,id).map(i64::from)),
            ("--cuda-lock-mem-clocks",|p,id|super::effective_memory_clock(p,id).map(i64::from)),
            ("--cuda-power-limits",|p,id|super::effective_power_limit(p,id).map(i64::from)),
        ])
    } else if profile.engine == "bzminer" {
        ("BzMiner",vec![
            ("--oc_lock_memory_clock",|p,id|super::effective_memory_clock(p,id).map(i64::from)),
            ("--oc_core_clock_offset",|p,id|super::effective_core_offset(p,id).map(i64::from)),
            ("--oc_memory_clock_offset",|p,id|super::effective_memory_offset(p,id).map(i64::from)),
        ])
    } else {
        ("SRBMiner",vec![
            ("--gpu-plimit0",|p,id|super::effective_power_limit(p,id).map(i64::from)),
            ("--gpu-cclock0",|p,id|super::effective_core_clock(p,id).map(i64::from)),
            ("--gpu-mclock0",|p,id|super::effective_memory_clock(p,id).map(i64::from)),
            ("--gpu-coffset0",|p,id|super::effective_core_offset(p,id).map(i64::from)),
            ("--gpu-moffset0",|p,id|super::effective_memory_offset(p,id).map(i64::from)),
            ("--gpu-fan0",|p,id|super::effective_fan(p,id).map(i64::from)),
        ])
    };
    for (flag,get) in getters {
        let values:Vec<_>=selected.iter().map(|id|get(profile,id)).collect();
        if flag=="--gpu-plimit0" {
            for value in values.iter().flatten() { validate_srb_power(&[flag.into(),value.to_string()],french)?; }
        }
        if values.iter().any(Option::is_some) && values.iter().any(Option::is_none) {
            return Err(if french {
                format!("{engine} : réglages partiels pour {flag}. Il faut une valeur par GPU sélectionné, dans le même ordre. Effacez tous les réglages pour ne pas envoyer cette option, ou utilisez des profils séparés pour régler seulement certaines cartes.")
            } else {
                format!("{engine}: partial GPU tuning for {flag}. A value is required for every selected GPU, in selection order. Clear all tuning to omit this option, or use separate profiles to tune only some cards.")
            });
        }
    }
    Ok(())
}

pub fn validate_srb_power(args: &[String], french: bool) -> Result<(), String> {
    for (index,arg) in args.iter().enumerate() {
        let (flag,inline) = arg.split_once('=').map_or((arg.as_str(),None),|(flag,value)|(flag,Some(value)));
        if !["--gpu-plimit", "--gpu-plimit0", "--gpu-plimit1"].contains(&flag) { continue; }
        let value = inline.or_else(|| args.get(index+1).map(String::as_str)).unwrap_or("");
        if value.is_empty() || value.split(',').any(|part| part.parse::<u32>().map_or(true,|n|n>1000)) {
            return Err(if french {
                format!("SRBMiner : limite de puissance invalide {value:?} pour {flag}. Saisissez un entier de 0 à 1000 W, ou effacez les réglages GPU. La puissance est en watts, pas en MHz : 2100 MHz correspond à la fréquence du cœur GPU. Vérifiez aussi les réglages globaux/anciens et les arguments avancés. Le pilote peut imposer une plage plus limitée.")
            } else {
                format!("SRBMiner: invalid power limit {value:?} for {flag}. Enter an integer from 0 to 1000 W, or clear GPU tuning. Power uses watts, not MHz: 2100 MHz belongs in GPU core clock. Also check global/legacy defaults and Advanced arguments. Your driver may require a narrower range.")
            });
        }
    }
    Ok(())
}

fn windows_quote(value: &str) -> String {
    if !value.is_empty() && !value.chars().any(|c| c.is_whitespace() || c=='"') { return value.into(); }
    let mut result = String::from("\"");
    let mut slashes = 0;
    for character in value.chars() {
        if character == '\\' { slashes += 1; continue; }
        result.push_str(&"\\".repeat(if character == '"' { slashes*2+1 } else { slashes }));
        result.push(character); slashes = 0;
    }
    result.push_str(&"\\".repeat(slashes*2)); result.push('"'); result
}

pub fn display_command(path: &str, args: &[String], windows: bool) -> String {
    std::iter::once(path).chain(args.iter().map(String::as_str)).map(|value| {
        if windows { windows_quote(value) }
        else { shlex::try_quote(value).map(|value|value.into_owned()).unwrap_or_else(|_|format!("{value:?}")) }
    }).collect::<Vec<_>>().join(" ")
}

pub fn is_error_line(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    ["invalid value", "error", "failed", "not supported", "no suitable", "permission denied", "cannot", "could not"]
        .iter().any(|pattern|line.contains(pattern))
}

pub fn exit_message(code: i32, french: bool, logs: &[LogLine]) -> String {
    let start = logs.iter().rposition(|line| line.stream == "system" && (line.text.starts_with("[MinerDesk] [USER]") || line.text.starts_with("[MinerDesk] [DEV TIP]"))).unwrap_or(0);
    let detail = logs[start..].iter().rev().find(|line|line.stream != "system" && is_error_line(&line.text))
        .map(|line|line.text.chars().take(512).collect::<String>());
    let prefix = if french { format!("Processus terminé avec le code {code}.") } else { format!("Miner exited with code {code}.") };
    match detail {
        Some(detail) => format!("{prefix} {detail}"),
        None => format!("{prefix} {}", if french { "Consultez la console du mineur pour les détails." } else { "See the miner console for details." }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MinerProfile, GpuTuning, build_engine_args};
    #[test] fn explicit_empty_tuning_does_not_restore_a_legacy_power_limit() {
        let mut profile=MinerProfile::default();
        profile.algorithm="pearlhash".into(); profile.pool="example.invalid:3360".into(); profile.wallet="fixture-wallet".into();
        profile.gpu_ids="0".into(); profile.core_clock=Some(2100); profile.power_limit=Some(2100);
        assert!(build_engine_args(&profile).unwrap_err().contains("2100"));
        let old:GpuTuning=serde_json::from_str(r#"{"selector":"0","core_clock":2200,"power_limit":null}"#).unwrap();
        assert!(!old.ignore_defaults); profile.gpu_tuning=vec![old];
        assert!(build_engine_args(&profile).is_err());
        profile.gpu_tuning=vec![GpuTuning{selector:"0".into(),ignore_defaults:true,core_clock:Some(2100),power_limit:None,fan:Some(60),..Default::default()}];
        profile=serde_json::from_str(&serde_json::to_string(&profile).unwrap()).unwrap();
        let args=build_engine_args(&profile).unwrap();
        assert!(!args.iter().any(|arg|arg.starts_with("--gpu-plimit")));
        assert!(args.windows(2).any(|pair|pair==["--gpu-cclock0","2100"]));
        assert!(args.windows(2).any(|pair|pair==["--gpu-fan0","60"]));
        profile.gpu_tuning.clear();profile.power_limit=None;profile.core_clock=None;profile.fan=None;
        assert!(!build_engine_args(&profile).unwrap().iter().any(|arg|arg.starts_with("--gpu-plimit")||arg.starts_with("--gpu-cclock")));
        profile.extra_args="--gpu-plimit0=2100".into();assert!(build_engine_args(&profile).is_err());
    }
    #[test] fn partially_cleared_multi_gpu_tuning_is_not_silently_discarded() {
        let mut profile=MinerProfile::default();profile.gpu_ids="0,1".into();
        profile.power_limit=Some(180);
        profile.gpu_tuning=vec![GpuTuning{selector:"0".into(),ignore_defaults:true,..Default::default()}];
        assert!(validate_gpu_selection(&profile,false).unwrap_err().contains("partial GPU tuning"));
        profile.power_limit=Some(2100);
        assert!(validate_gpu_selection(&profile,false).unwrap_err().contains("invalid power limit"));
        profile.power_limit=None;assert!(validate_gpu_selection(&profile,false).is_ok());
    }
    #[test] fn power_validation_checks_lists_aliases_and_advanced_arguments() {
        for flag in ["--gpu-plimit", "--gpu-plimit0", "--gpu-plimit1"] {
            for value in ["2100", "120,2100", "-1", "1.5", "abc", ""] {
                let error = validate_srb_power(&[flag.into(),value.into()],false).unwrap_err();
                assert!(error.contains("watts, not MHz")); assert!(error.contains(flag));
            }
            assert!(validate_srb_power(&[flag.into(),"0,180,1000".into()],false).is_ok());
        }
        assert!(validate_srb_power(&["--gpu-plimit0=2100".into()],false).is_err());
        assert!(validate_srb_power(&["--gpu-cclock0".into(),"2745".into()],false).is_ok());
        assert!(validate_srb_power(&["--gpu-plimit0".into(),"2100".into()],true).unwrap_err().contains("puissance"));
    }
    #[test] fn command_display_quotes_arguments_without_changing_argument_boundaries() {
        let args=vec!["--api-rig-name".into(),"PRL Eco".into(),"wallet'quoted".into(),"$literal".into(),"".into()];
        let displayed=display_command("/home/test/miner folder/SRBMiner-MULTI",&args,false);
        let expected=std::iter::once("/home/test/miner folder/SRBMiner-MULTI".into()).chain(args).collect::<Vec<String>>();
        assert_eq!(shlex::split(&displayed).unwrap(),expected);
        assert_eq!(windows_quote("PRL Eco"),"\"PRL Eco\"");
        assert_eq!(windows_quote("C:\\miner folder\\"),"\"C:\\miner folder\\\\\"");
        assert_eq!(windows_quote("a\"b"),"\"a\\\"b\"");
    }
    #[test] fn exit_errors_use_current_session_output_in_the_selected_language() {
        let line=|stream:&str,text:&str|LogLine{ts:0,stream:stream.into(),text:text.into()};
        let logs=vec![line("stderr","old failure"),line("system","[MinerDesk] [USER] fixture"),line("stdout","--gpu-plimit has invalid value '2100': use integers from 0 to 1000")];
        let error=exit_message(1,true,&logs); assert!(error.contains("Processus terminé")); assert!(error.contains("invalid value '2100'")); assert!(!error.contains("old failure"));
        assert!(exit_message(1,false,&logs[..1]).starts_with("Miner exited with code 1."));
        assert!(!exit_message(0,false,&logs[1..2]).contains("old failure"));
    }
}
