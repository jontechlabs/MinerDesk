# Windows miner GPU detection and access refusals

MinerDesk obtains GPU selectors from the selected mining executable, using its device-listing command. A card appearing in the Windows or NVIDIA inventory does not verify the mining engine's device IDs. With mixed AMD and NVIDIA adapters, the miner's global IDs may differ from NVIDIA's indices.

If Windows refuses the executable with error 5, MinerDesk shows that native error directly. This error can have several causes; it does not prove antivirus involvement. The full executable path and device-listing output remain under **Manual IDs / diagnostics**. Saved GPU IDs and tuning are preserved; system fallback indices are never substituted for engine IDs.

1. Check **Windows Security → Virus & threat protection → Protection history** for an event affecting the exact executable path. Record the detection name and affected resource. If there is no matching event, check the file's access permissions instead.
2. If a downloaded engine was quarantined, verify its source and the affected file before deciding whether to restore or allow it. Use the engine's official release, not an unofficial repack. An exclusion does not restore an already quarantined file.
3. MinerDesk's optional Defender folder exclusion applies to its managed miner directory, normally `C:\ProgramData\MinerDesk\miners`. A manually selected engine elsewhere is outside that exclusion. A folder exclusion normally covers its files and subfolders; it is unnecessary to add a process exclusion merely to exclude the executable itself.
4. A configured antivirus exclusion does not guarantee that every Windows protection permits the file. Some ASR rules, application-control policies and other protection layers use different controls. Inspect the actual event before changing settings. Do not disable Defender or add blanket drive, extension or process exclusions to troubleshoot a single engine.
5. For a confirmed false positive, submit the affected official engine file to Microsoft for analysis, or report it to the engine maintainer. Each engine update changes the binary, and signing MinerDesk itself does not sign third-party mining executables.

After resolving the refusal, click **Refresh** in the GPU selector. This diagnostic action does not start mining. Choose the verified engine ID for the desired card, save and start the profile when ready.

Microsoft references: [exclusion scope and limitations](https://learn.microsoft.com/en-us/defender-endpoint/microsoft-defender-antivirus-exclusions-overview), [exclusion configuration](https://learn.microsoft.com/en-us/defender-endpoint/microsoft-defender-antivirus-exclusions-configure), [protection history and allowed threats](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/virus-and-threat-protection-in-the-windows-security-app), [submit a file for analysis](https://www.microsoft.com/en-us/wdsi/filesubmission).
