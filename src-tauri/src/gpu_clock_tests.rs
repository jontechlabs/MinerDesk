use super::*;

fn profile(engine: &str) -> MinerProfile {
    MinerProfile { engine:engine.into(), algorithm:"pearlhash".into(), pool:"pool.example:3333".into(), wallet:"fixture-wallet".into(), gpu_ids:"2,0".into(),
        gpu_tuning:vec![
            GpuTuning{selector:"2".into(),memory_clock:Some(810),core_offset:Some(-100),memory_offset:Some(500),..Default::default()},
            GpuTuning{selector:"0".into(),memory_clock:Some(5001),core_offset:Some(150),memory_offset:Some(-200),..Default::default()},
        ], ..Default::default() }
}
fn option(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a|a==flag).map(|i|args[i+1].clone())
}

#[test] fn new_clocks_follow_engine_gpu_order_and_skip_conventions() {
    let cases=[
        ("srbminer","--gpu-mclock0","--gpu-coffset0","--gpu-moffset0","810,5001","-100,150","500,-200"),
        ("lolminer","--mclk","--coff","--moff","5001,*,810","150,*,-100","-200,*,500"),
        ("rigel","--lock-mclock","--cclock","--mclock","5001,_,810","150,_,-100","-200,_,500"),
    ];
    for (engine,mem,coreoff,memoff,memvalues,corevalues,offsetvalues) in cases {
        let args=build_engine_args(&profile(engine)).unwrap();
        assert_eq!(option(&args,mem).as_deref(),Some(memvalues),"{engine}");
        assert_eq!(option(&args,coreoff).as_deref(),Some(corevalues),"{engine}");
        assert_eq!(option(&args,memoff).as_deref(),Some(offsetvalues),"{engine}");
    }
    let args=build_engine_args(&profile("bzminer")).unwrap();
    for (flag,values) in [("--oc_lock_memory_clock",["810","5001"]),("--oc_core_clock_offset",["-100","150"]),("--oc_memory_clock_offset",["500","-200"])] {
        let i=args.iter().position(|a|a==flag).unwrap();assert_eq!(&args[i+1..i+3],&values);
    }
    let args=build_engine_args(&profile("npminer")).unwrap();
    assert_eq!(option(&args,"--cuda-lock-mem-clocks").as_deref(),Some("810,5001"));
    assert!(!args.iter().any(|a|a.contains("offset")));
    let args=build_engine_args(&profile("lpminer")).unwrap();
    assert!(!args.iter().any(|a|a.contains("memory")||a.contains("mclock")||a.contains("offset")));
}

#[test] fn global_locks_and_negative_offsets_are_omitted_when_blank() {
    for (engine,mem,coreoff,memoff) in [("srbminer","--gpu-mclock0","--gpu-coffset0","--gpu-moffset0"),("lolminer","--mclk","--coff","--moff"),("rigel","--lock-mclock","--cclock","--mclock"),("bzminer","--oc_lock_memory_clock","--oc_core_clock_offset","--oc_memory_clock_offset")] {
        let mut p=profile(engine);p.gpu_ids.clear();p.memory_clock=Some(810);p.core_offset=Some(-100);p.memory_offset=Some(-200);
        let args=build_engine_args(&p).unwrap();assert_eq!(option(&args,mem).as_deref(),Some("810"));assert_eq!(option(&args,coreoff).as_deref(),Some("-100"));assert_eq!(option(&args,memoff).as_deref(),Some("-200"));
        p.memory_clock=None;p.core_offset=None;p.memory_offset=None;
        let args=build_engine_args(&p).unwrap();assert!(!args.contains(&mem.into())&&!args.contains(&coreoff.into())&&!args.contains(&memoff.into()));
    }
}

#[test] fn old_profiles_default_to_no_new_tuning_and_explicit_clears_survive_roundtrip() {
    let old:MinerProfile=serde_json::from_str(r#"{"engine":"srbminer","gpu_ids":"0","core_clock":2200,"gpu_tuning":[{"selector":"0","core_clock":2300}]}"#).unwrap();
    assert_eq!(effective_memory_clock(&old,"0"),None);assert_eq!(effective_core_offset(&old,"0"),None);assert_eq!(effective_core_clock(&old,"0"),Some(2300));
    let mut p=profile("srbminer");p.gpu_ids="0".into();p.memory_clock=Some(810);p.core_offset=Some(-100);p.memory_offset=Some(-200);
    p.gpu_tuning=vec![GpuTuning{selector:"0".into(),ignore_defaults:true,..Default::default()}];
    let restored:MinerProfile=serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    let args=build_engine_args(&restored).unwrap();assert!(!args.iter().any(|a|a=="--gpu-mclock0"||a=="--gpu-coffset0"||a=="--gpu-moffset0"));
    assert_eq!(restored.memory_clock,Some(810));assert_eq!(restored.wallet,p.wallet);
}

#[test] fn incomplete_memory_lists_and_invalid_fan_have_actionable_errors() {
    for engine in ["srbminer","npminer"] {
        let mut p=profile(engine);p.gpu_tuning[0].memory_clock=None;
        let error=build_engine_args_with_language(&p,true).unwrap_err();assert!(error.contains("réglages partiels"));assert!(error.contains(if engine=="srbminer"{"--gpu-mclock0"}else{"--cuda-lock-mem-clocks"}));
    }
    let mut p=profile("srbminer");p.gpu_tuning[0].core_offset=None;
    assert!(build_engine_args(&p).unwrap_err().contains("--gpu-coffset0"));
    p.gpu_tuning[0].core_offset=Some(0);p.fan=Some(101);
    assert!(build_engine_args_with_language(&p,true).unwrap_err().contains("Ventilateur"));
}
