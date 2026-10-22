use super::*;

/// PeakMiner accepts coin names rather than MinerDesk's generic algorithm name.
fn coin(value: &str, french: bool) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pearl" | "pearlhash" | "prl" => Ok("pearl"),
        "quantus" | "qtc" | "quan" => Ok("quantus"),
        "btx" => Ok("btx"),
        "csd" | "sha256d" | "sha-256d" => Ok("csd"),
        "midstate" | "mds" => Ok("midstate"),
        "alphanumeric" | "alp" => Ok("alphanumeric"),
        "parano1d" | "noid" => Ok("parano1d"),
        _ => Err(if french {
            "PeakMiner : choisissez pearl / pearlhash (PRL), quantus, btx, csd, midstate, alphanumeric ou parano1d dans le champ Algorithme."
        } else {
            "PeakMiner: choose pearl / pearlhash (PRL), quantus, btx, csd, midstate, alphanumeric or parano1d in the Algorithm field."
        }.into()),
    }
}

pub(super) fn support_coin(value: &str) -> Option<&'static str> {
    match coin(value,false).ok()? { "pearl" => Some("PRL"), "quantus" => Some("QTC"), _ => None }
}

fn selection(value: &str, french: bool) -> Result<Vec<String>, String> {
    let selected = gpu_tokens(value);
    if selected.len() == 1 && selected[0].eq_ignore_ascii_case("all") { return Ok(vec![]); }
    let invalid = || if french {
        "PeakMiner : utilisez des indices GPU distincts (0,1) ou des adresses PCI distinctes (01:00.0,0000:03:00.0), sans mélanger les deux. Confirmez-les dans la console du mineur."
    } else {
        "PeakMiner: use distinct GPU indices (0,1) or distinct PCI addresses (01:00.0,0000:03:00.0), without mixing the two. Confirm them in the miner console."
    }.to_string();
    let pci = Regex::new(r"(?i)^(?:([0-9a-f]{4,8}):)?([0-9a-f]{2}):([0-9a-f]{2})\.([0-7])$").unwrap();
    let mut seen = HashSet::new();
    let mut kind = None;
    for id in &selected {
        let (current, key) = if !id.is_empty() && id.bytes().all(|c|c.is_ascii_digit()) {
            let number = id.parse::<u32>().map_err(|_|invalid())?;
            if number > 4095 { return Err(invalid()); }
            (false, format!("index:{number}"))
        } else if let Some(parts) = pci.captures(id) {
            let domain = u32::from_str_radix(parts.get(1).map_or("0000",|p|p.as_str()),16).map_err(|_|invalid())?;
            let bus = u32::from_str_radix(&parts[2],16).map_err(|_|invalid())?;
            let device = u32::from_str_radix(&parts[3],16).map_err(|_|invalid())?;
            if domain > 0xffff || device > 0x1f { return Err(invalid()); }
            (true, format!("pci:{domain:04x}:{bus:02x}:{device:02x}.{}",&parts[4]))
        } else { return Err(invalid()); };
        if kind.is_some_and(|previous| previous != current) || !seen.insert(key) { return Err(invalid()); }
        kind = Some(current);
    }
    Ok(selected)
}

// No documented standalone device-list command exists. Shared values are safe
// without assuming that an OS adapter index is PeakMiner's OC index.
fn shared<T: Copy + PartialEq + std::fmt::Display>(
    profile: &MinerProfile, selected: &[String], global: Option<T>,
    get: impl Fn(&MinerProfile,&str)->Option<T>, flag: &str, french: bool,
) -> Result<Option<String>,String> {
    if selected.is_empty() { return Ok(global.map(|v|v.to_string())); }
    let first = get(profile,&selected[0]);
    if selected.iter().any(|id|get(profile,id)!=first) {
        return Err(if french {
            format!("PeakMiner : {flag} utilise ici un réglage GPU commun. Effacez les anciens réglages par GPU puis configurez les réglages communs, ou utilisez des profils séparés.")
        } else {
            format!("PeakMiner: {flag} uses shared GPU settings here. Clear previous per-GPU tuning and configure shared settings, or use separate profiles.")
        });
    }
    Ok(first.map(|v|v.to_string()))
}

pub(super) fn build_args(p: &MinerProfile, wallet: String, french: bool) -> Result<Vec<String>,String> {
    if p.pool.trim().is_empty() || wallet.is_empty() { return Err(if french { "PeakMiner : pool et adresse publique requis." } else { "PeakMiner: pool and public wallet are required." }.into()); }
    let coin = coin(&p.algorithm,french)?;
    let selected = selection(&p.gpu_ids,french)?;
    let mut args = vec!["--coin".into(),coin.into(),"--url".into(),p.pool.trim().into(),"--user".into(),wallet];
    if !p.password.trim().is_empty() { args.extend(["--password".into(),p.password.trim().into()]); }
    if !p.worker.trim().is_empty() { args.extend(["--worker".into(),p.worker.trim().into()]); }
    if !selected.is_empty() { args.extend(["--devices".into(),selected.join(",")]); }
    macro_rules! tuning {
        ($field:ident,$getter:ident,$flag:literal) => {
            if let Some(value)=shared(p,&selected,p.$field,$getter,$flag,french)? { args.extend([$flag.into(),value]); }
        }
    }
    tuning!(core_clock,effective_core_clock,"--gpu-lcore");
    tuning!(memory_clock,effective_memory_clock,"--gpu-lmem");
    tuning!(core_offset,effective_core_offset,"--gpu-core");
    tuning!(memory_offset,effective_memory_offset,"--gpu-mem");
    tuning!(power_limit,effective_power_limit,"--gpu-power");
    tuning!(fan,effective_fan,"--gpu-fan");
    // Never inherit the engine's fixed default port or enable its write API.
    let address=p.api_port.filter(|port|*port!=0).map(|port|format!("127.0.0.1:{port}")).unwrap_or_else(||"0".into());
    args.extend(["--api-port".into(),address,"--status-interval".into(),"10".into(),"--no-color".into(),"--no-tips".into()]);
    Ok(args)
}

pub(super) fn verify_download(asset: &GithubAsset, bytes: &[u8]) -> Result<(),String> {
    use sha2::{Digest,Sha256};
    let expected=asset.digest.as_deref().and_then(|digest|digest.strip_prefix("sha256:"))
        .filter(|hash|hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit()))
        .ok_or_else(||"PeakMiner: GitHub did not provide a valid SHA256 digest; download cancelled.".to_string())?;
    if format!("{:x}",Sha256::digest(bytes)).eq_ignore_ascii_case(expected) { Ok(()) }
    else { Err("PeakMiner: SHA256 mismatch; download cancelled before extraction.".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> MinerProfile {
        MinerProfile {engine:"peakminer".into(),algorithm:"pearlhash".into(),pool:"pool.example:3360".into(),wallet:"prl1-fixture.worker".into(),
            worker:"Rig with spaces".into(),password:"x;d=524288".into(),api_port:Some(21552),..Default::default()}
    }
    fn option(args:&[String],flag:&str)->Option<String> { args.iter().position(|arg|arg==flag).map(|index|args[index+1].clone()) }
    #[test] fn official_arguments_preserve_logins_workers_and_local_api() {
        let mut p=profile();p.gpu_ids="2,0".into();p.extra_args="--keepalive".into();
        let args=build_engine_args(&p).unwrap();
        for (flag,value) in [("--coin","pearl"),("--url","pool.example:3360"),("--user","prl1-fixture.worker"),("--worker","Rig with spaces"),("--password","x;d=524288"),("--devices","2,0"),("--api-port","127.0.0.1:21552")] { assert_eq!(option(&args,flag).as_deref(),Some(value)); }
        assert!(args.contains(&"--keepalive".into()));assert!(!args.contains(&"--disable-cpu".into()));assert!(!args.contains(&"--api-control".into()));
        p.api_port=None;assert_eq!(option(&build_engine_args(&p).unwrap(),"--api-port").as_deref(),Some("0"));
        for (input,expected) in [("PRL","pearl"),("quantus","quantus"),("QTC","quantus"),("mds","midstate"),("sha256d","csd"),("ALP","alphanumeric"),("noid","parano1d"),("btx","btx")] {
            p.algorithm=input.into();assert_eq!(option(&build_engine_args(&p).unwrap(),"--coin").as_deref(),Some(expected));
        }
        p.algorithm="unsupported".into();assert!(build_engine_args_with_language(&p,true).unwrap_err().contains("Algorithme"));
    }
    #[test] fn selection_rejects_mixed_aliases_duplicates_and_invalid_pci() {
        for value in ["0,0","01,1","0,01:00.0","01:00.0,0000:01:00.0","01:20.0","01:00.8","0;all","-1","+1","4096"] {
            assert!(selection(value,false).is_err(),"{value}");
        }
        for value in ["2,0","01:00.0,0000:03:00.0","00000000:01:00.0"] { assert!(!selection(value,false).unwrap().is_empty()); }
        assert!(selection("all",false).unwrap().is_empty());
    }
    #[test] fn shared_tuning_uses_correct_units_and_explicit_clearing() {
        let mut p=profile();p.gpu_ids="1".into();p.core_clock=Some(2200);p.memory_clock=Some(810);p.core_offset=Some(-100);p.memory_offset=Some(500);p.power_limit=Some(160);p.fan=Some(60);
        let args=build_engine_args(&p).unwrap();
        for (flag,value) in [("--gpu-lcore","2200"),("--gpu-lmem","810"),("--gpu-core","-100"),("--gpu-mem","500"),("--gpu-power","160"),("--gpu-fan","60")] { assert_eq!(option(&args,flag).as_deref(),Some(value)); }
        p.gpu_tuning=vec![GpuTuning{selector:"1".into(),ignore_defaults:true,..Default::default()}];
        let args=build_engine_args(&p).unwrap();assert!(args.iter().all(|arg|!arg.starts_with("--gpu-")));
        p.gpu_ids="1,0".into();assert!(build_engine_args_with_language(&p,true).unwrap_err().contains("réglage GPU commun"));
        p.gpu_tuning.clear();p.gpu_ids="01:00.0".into();assert_eq!(option(&build_engine_args(&p).unwrap(),"--gpu-lmem").as_deref(),Some("810"));
        p.fan=Some(101);assert!(build_engine_args(&p).unwrap_err().contains("fan"));
        let args=build_engine_args(&profile()).unwrap();assert!(args.iter().all(|arg|!arg.starts_with("--gpu-")));
    }
    #[test] fn download_selects_only_official_platform_archive_and_original_executable() {
        #[cfg(windows)] let valid="peakminer-2.20.0-windows-x86_64.zip";
        #[cfg(not(windows))] let valid="peakminer-2.20.0.tar.gz";
        assert!(asset_matches("peakminer",valid));
        for invalid in ["peakminer-source.tar.gz","peakminer-2.20.0-linux-x86_64","peakminer-2.20.0-windows-arm64.zip","peakminer-2.20.0-arm64.tar.gz","peakminer-2.20.0.tar.gz.sig"] { assert!(!asset_matches("peakminer",invalid)); }
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join(engine_executable_names("peakminer")[0]);fs::write(&path,b"fixture").unwrap();
        assert_eq!(find_engine_executable(dir.path(),"peakminer"),Some(path));
        assert!(engine_specs().iter().any(|spec|spec.id=="peakminer"&&spec.auto_download&&spec.github_repo==Some("peakminer/peakminer")));
        assert!(engine_list_device_args("peakminer").is_none());
        let fallback=discover_gpus_with_fallback(&profile(),||vec![GpuDevice{selector:"0".into(),name:"Reference GPU".into(),vendor:"NVIDIA".into(),pci_bus:None}]);
        assert!(!fallback.selectors_verified);assert!(fallback.selection_hint.contains("PCI"));
    }
    #[test] fn download_digest_fails_closed_on_tampering_or_missing_hash() {
        let mut asset=GithubAsset{name:"fixture".into(),browser_download_url:String::new(),digest:Some("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into())};
        assert!(verify_download(&asset,b"abc").is_ok());assert!(verify_download(&asset,b"changed").is_err());
        asset.digest=None;assert!(verify_download(&asset,b"abc").is_err());
    }
    #[test] fn support_preserves_embedded_worker_and_does_not_edit_original_profile() {
        let original=profile();let mut support=original.clone();
        assert_eq!(apply_dev_tip_wallets(&mut support),Some("PRL"));assert_eq!(support.wallet,format!("{DEV_WALLET_PRL}.worker"));
        assert_eq!(original.wallet,"prl1-fixture.worker");
        let args=build_engine_args(&support).unwrap();assert_eq!(option(&args,"--worker").as_deref(),Some("Rig with spaces"));
        support=profile();support.algorithm="QTC".into();support.wallet="QUANTUS/worker".into();
        assert_eq!(apply_dev_tip_wallets(&mut support),Some("QTC"));assert_eq!(support.wallet,format!("{DEV_WALLET_QTC}/worker"));
        support.algorithm="csd".into();support.wallet="0xuser".into();assert_eq!(apply_dev_tip_wallets(&mut support),None);assert_eq!(support.wallet,"0xuser");
    }
}
