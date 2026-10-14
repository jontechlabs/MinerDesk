use super::*;

/// OneZeroMiner does not document a standalone device-listing command. Until
/// its device ordering can be verified, only a shared tuning value is emitted;
/// an inherited or imported per-card override must never tune another card.
fn shared_tuning<T: Copy + PartialEq + std::fmt::Display>(
    profile: &MinerProfile, selected: &[String], global: Option<T>,
    get: impl Fn(&MinerProfile, &str) -> Option<T>, flag: &str, french: bool,
) -> Result<Option<String>, String> {
    if selected.is_empty() { return Ok(global.map(|value| value.to_string())); }
    let first = get(profile, &selected[0]);
    if selected.iter().any(|id| get(profile,id) != first) {
        return Err(if french {
            format!("OneZeroMiner : {flag} utilise un réglage commun dans MinerDesk. Utilisez la même valeur sur tous les GPU sélectionnés, ou effacez les réglages GPU et configurez les valeurs par défaut communes.")
        } else {
            format!("OneZeroMiner: {flag} uses shared tuning in MinerDesk. Set the same value on every selected GPU, or clear GPU tuning and configure shared GPU defaults.")
        });
    }
    Ok(first.map(|value|value.to_string()))
}

pub(super) fn build_args(p: &MinerProfile, wallet: String, french: bool) -> Result<Vec<String>,String> {
    if p.algorithm.trim().is_empty() || p.pool.trim().is_empty() || wallet.is_empty() {
        return Err("Algorithm, pool and wallet are required".into());
    }
    let selected=gpu_tokens(&p.gpu_ids);
    let mut seen=HashSet::new();
    if selected.iter().any(|id| id.parse::<u32>().map_or(true,|index|index>4095) || !seen.insert(id.parse::<u32>().unwrap_or(u32::MAX))) {
        return Err(if french { "OneZeroMiner : utilisez des identifiants GPU numériques distincts (par exemple 0,1), confirmés dans sa console." }
            else { "OneZeroMiner: use distinct numeric GPU IDs (for example 0,1), confirmed in its console." }.into());
    }
    let algo=if p.algorithm.trim().eq_ignore_ascii_case("pearl") { "pearlhash" } else { p.algorithm.trim() };
    let mut args=vec!["--algo".into(),algo.into(),"--pool".into(),p.pool.trim().into(),"--wallet".into(),wallet];
    if !p.password.trim().is_empty() { args.extend(["--pass".into(),p.password.trim().into()]); }
    if !p.worker.trim().is_empty() { args.extend(["--worker".into(),p.worker.trim().into()]); }
    if !selected.is_empty() { args.extend(["--devices".into(),selected.join(",")]); }
    macro_rules! tuning {
        ($field:ident,$getter:ident,$flag:literal) => {
            if let Some(value)=shared_tuning(p,&selected,p.$field,$getter,$flag,french)? { args.extend([$flag.into(),value]); }
        }
    }
    tuning!(core_clock,effective_core_clock,"--cclk");
    tuning!(memory_clock,effective_memory_clock,"--mclk");
    tuning!(core_offset,effective_core_offset,"--coff");
    tuning!(memory_offset,effective_memory_offset,"--moff");
    tuning!(power_limit,effective_power_limit,"--pl");
    tuning!(fan,effective_fan,"--fan");
    if let Some(port)=p.api_port { args.extend(["--api-host".into(),"127.0.0.1".into(),"--api-port".into(),port.to_string()]); }
    else { args.push("--api-disable".into()); }
    args.push("--disable-telemetry".into());
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> MinerProfile {
        MinerProfile {engine:"onezerominer".into(),algorithm:"pearlhash".into(),pool:"pool.example:3360".into(),wallet:"prl1-fixture".into(),
            worker:"Rig with spaces".into(),password:"x".into(),api_port:Some(21551),..Default::default()}
    }
    fn option(args:&[String],flag:&str)->Option<String> { args.iter().position(|arg|arg==flag).map(|index|args[index+1].clone()) }
    #[test] fn official_command_maps_wallet_worker_pool_api_and_both_coins() {
        let mut p=profile();p.gpu_ids="2,0".into();p.extra_args="--kernel 2".into();
        let args=build_engine_args(&p).unwrap();
        for (flag,value) in [("--algo","pearlhash"),("--pool","pool.example:3360"),("--wallet","prl1-fixture"),("--worker","Rig with spaces"),("--pass","x"),("--devices","2,0"),("--api-host","127.0.0.1"),("--api-port","21551"),("--kernel","2")] {
            assert_eq!(option(&args,flag).as_deref(),Some(value));
        }
        assert!(args.contains(&"--disable-telemetry".into()));assert!(!args.contains(&"--disable-cpu".into()));
        p.algorithm="pearl".into();assert_eq!(option(&build_engine_args(&p).unwrap(),"--algo").as_deref(),Some("pearlhash"));
        p.algorithm="quantus".into();p.api_port=None;
        let args=build_engine_args(&p).unwrap();assert_eq!(option(&args,"--algo").as_deref(),Some("quantus"));
        assert!(args.contains(&"--api-disable".into()));assert!(!args.contains(&"--api-port".into()));
        assert!(engine_specs().iter().any(|spec|spec.id=="onezerominer"&&spec.auto_download&&spec.github_repo==Some("OneZeroMiner/onezerominer")));
    }
    #[test] fn tuning_preserves_units_offsets_and_explicit_clears() {
        let mut p=profile();p.gpu_ids="1".into();p.core_clock=Some(2200);p.memory_clock=Some(810);p.core_offset=Some(-100);p.memory_offset=Some(500);p.power_limit=Some(160);p.fan=Some(60);
        let args=build_engine_args(&p).unwrap();
        for (flag,value) in [("--cclk","2200"),("--mclk","810"),("--coff","-100"),("--moff","500"),("--pl","160"),("--fan","60")] { assert_eq!(option(&args,flag).as_deref(),Some(value)); }
        p.gpu_tuning=vec![GpuTuning{selector:"1".into(),ignore_defaults:true,..Default::default()}];
        let args=build_engine_args(&p).unwrap();for flag in ["--cclk","--mclk","--coff","--moff","--pl","--fan"] { assert!(option(&args,flag).is_none()); }
        p.gpu_ids="1,0".into();assert!(build_engine_args_with_language(&p,true).unwrap_err().contains("réglage commun"));
        p.gpu_tuning.clear();assert_eq!(option(&build_engine_args(&p).unwrap(),"--cclk").as_deref(),Some("2200"));
        p.gpu_ids="0,0".into();assert!(build_engine_args(&p).is_err());p.gpu_ids="1:0".into();assert!(build_engine_args(&p).is_err());
        p.gpu_ids.clear();p.fan=Some(101);assert!(build_engine_args(&p).unwrap_err().contains("fan"));
    }
    #[test] fn only_standalone_platform_assets_are_downloaded() {
        #[cfg(windows)] let valid="onezerominer-win64-1.8.1.zip";
        #[cfg(not(windows))] let valid="onezerominer-linux-1.8.1.tar.gz";
        assert!(asset_matches("onezerominer",valid));
        assert!(!asset_matches("onezerominer","onezerominer-1.8.1.tar.gz"));
        assert!(!asset_matches("onezerominer","onezerominer-linux-arm64-1.8.1.tar.gz"));
        assert!(!asset_matches("onezerominer","onezerominer-linux-aarch64-1.8.1.tar.gz"));
        assert!(!asset_matches("onezerominer","onezerominer-win64-1.8.1.zip.sig"));
        let dir=tempfile::tempdir().unwrap();
        let name=engine_executable_names("onezerominer")[0];let path=dir.path().join(name);fs::write(&path,b"fixture").unwrap();
        assert_eq!(find_engine_executable(dir.path(),"onezerominer"),Some(path));
        assert!(engine_list_device_args("onezerominer").is_none());
        let fallback=discover_gpus_with_fallback(&profile(),||vec![GpuDevice{selector:"0".into(),name:"NVIDIA RTX fixture".into(),vendor:"NVIDIA".into(),pci_bus:None}]);
        assert!(!fallback.selectors_verified);assert_eq!(fallback.source,"system");assert!(fallback.selection_hint.contains("console"));
    }
    #[test] fn support_switch_keeps_worker_and_gpu_configuration() {
        let mut tip=profile();assert_eq!(apply_dev_tip_wallets(&mut tip),Some("PRL"));
        let args=build_engine_args(&tip).unwrap();assert_eq!(option(&args,"--wallet").as_deref(),Some(DEV_WALLET_PRL));
        assert_eq!(option(&args,"--worker").as_deref(),Some("Rig with spaces"));
    }
}
