#!/usr/bin/env python3
"""Dependency-free source checks. Not a substitute for makensis/PowerShell.

The small NSIS line tokenizer checks quoting/argument boundaries using the
published NSIS line parsing rules. It deliberately does not compile an installer.
Run: python tests/verify_installer_sources.py [--old-root <0.7.16 source root>]
"""
from __future__ import annotations
import argparse
import json
import re
from pathlib import Path


def nsis_tokens(line: str) -> list[str]:
    result = []
    i = 0
    while i < len(line):
        while i < len(line) and line[i].isspace():
            i += 1
        if i == len(line) or line[i] in ';#':
            break
        quote = line[i] if line[i] in "\"'`" else None
        if quote:
            i += 1
        value = []
        closed = quote is None
        while i < len(line):
            if line.startswith('$\\', i) and i + 2 < len(line) and line[i + 2] in "\"'`":
                value.append(line[i + 2]); i += 3
                continue
            if quote and line[i] == quote:
                i += 1; closed = True
                break
            if not quote and line[i].isspace():
                break
            value.append(line[i]); i += 1
        if not closed:
            raise ValueError(f'Unclosed NSIS quote: {line}')
        result.append(''.join(value))
    return result


def ps_brackets(text: str) -> None:
    """Check delimiters outside comments/literals. This is NOT a PS parser."""
    stack = []
    i = 0
    while i < len(text):
        if text.startswith('<#', i):
            end = text.find('#>', i + 2)
            assert end >= 0, 'Unclosed PowerShell block comment'
            i = end + 2; continue
        if text[i] == '#':
            end = text.find('\n', i)
            i = len(text) if end < 0 else end + 1
            continue
        if text.startswith(("@'", '@"'), i):
            delimiter = text[i+1] + '@'
            end = re.search(r'^' + re.escape(delimiter), text[i+2:], re.M)
            assert end, 'Unclosed PowerShell here-string'
            i += 2 + end.end(); continue
        if text[i] in "\"'":
            quote = text[i]; i += 1
            while i < len(text):
                if quote == '"' and text[i] == '`':
                    i += 2; continue
                if text[i] == quote:
                    if i+1 < len(text) and text[i+1] == quote:
                        i += 2; continue
                    i += 1; break
                i += 1
            else:
                raise AssertionError('Unclosed PowerShell string')
            continue
        if text[i] == '`':
            i += 2; continue
        if text[i] in '([{':
            stack.append((text[i], text.count('\n', 0, i)+1))
        if text[i] in ')]}':
            assert stack and stack[-1][0] == {')': '(', ']': '[', '}': '{'}[text[i]], f'Unmatched {text[i]} near line {text.count(chr(10), 0, i)+1}'
            stack.pop()
        i += 1
    assert not stack, f'Unclosed PowerShell delimiters: {stack}'


def expanded_hooks(text: str) -> list[str]:
    macros = {}
    top = []
    lines = iter(text.splitlines())
    for line in lines:
        token = nsis_tokens(line)
        if token and token[0].lower() == '!macro':
            body = []
            for b in lines:
                if b.strip().lower() == '!macroend':
                    break
                body.append(b)
            macros[token[1]] = (token[2:], body)
        else:
            top.append(line)
    def expand(lines_to_expand):
        output = []
        for line in lines_to_expand:
            tok = nsis_tokens(line)
            if tok and tok[0] == '!insertmacro' and tok[1] in macros:
                names, body = macros[tok[1]]
                assert len(names) == len(tok[2:])
                expanded = []
                for b in body:
                    for key, value in zip(names, tok[2:]):
                        b = b.replace('${' + key + '}', value)
                    expanded.append(b)
                output += expand(expanded)
            else:
                output.append(line)
        return output
    return expand(top)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--old-root', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    checks = []
    def ok(message):
        checks.append(message)
        print('PASS:', message)
    hooks = (root/'src-tauri/windows/hooks.nsh').read_text()
    for line in hooks.splitlines():
        nsis_tokens(line)
    ok('Every hook source line has balanced NSIS quoting')
    plugin_calls = [nsis_tokens(x) for x in hooks.splitlines() if x.strip().startswith('nsExec::')]
    assert len(plugin_calls) == 1
    assert len(plugin_calls[0]) == 3 and plugin_calls[0][1] == '/TIMEOUT=120000'
    assert ' -File ' in plugin_calls[0][2] and ' -Command ' not in plugin_calls[0][2]
    ok('One shared nsExec call: one timeout option and exactly one complete command argument')
    assert 'Pop $MdRawCode\n  Pop $MdOutput' in hooks
    ok('Both plugin stack values are consumed immediately')
    assert '$COMMONAPPDATA' not in hooks
    assert 'SetShellVarContext all' in hooks and '$APPDATA\\MinerDesk' in hooks
    ok('ProgramData path uses defined NSIS shell variables')
    lines = expanded_hooks(hooks)
    functions = {}
    calls = []
    current = None
    logic = []
    for line in lines:
        t = nsis_tokens(line)
        if not t: continue
        if t[0] == 'Function':
            assert current is None
            current = t[1]
            assert current not in functions, f'Duplicate function {current}'
            functions[current] = []
        elif t[0] == 'FunctionEnd':
            assert current is not None and not logic, f'Unbalanced logic in {current}'
            current = None
        elif current:
            functions[current].append(t)
            if t[0] in ('${If}', '${IfNot}'):
                logic.append(t[0])
            elif t[0] in ('${Else}', '${ElseIf}', '${ElseIfNot}'):
                assert logic
            elif t[0] == '${EndIf}':
                assert logic
                logic.pop()
            elif t[0] == 'Call':
                calls.append(t[1])
    assert current is None
    assert all(c in functions for c in calls), calls
    assert '.onGUIInit' not in functions
    ok(f'{len(functions)} expanded functions have unique names, balanced conditionals and resolved calls')
    for name, body in functions.items():
        labels = {t[0][:-1] for t in body if t[0].endswith(':')}
        for t in body:
            jumps = []
            if t[0] == 'Goto': jumps = t[1:]
            if t[0] == 'IfFileExists': jumps = t[2:]
            if t[0] == 'IfSilent': jumps = t[1:]
            if t[0] == 'StrCmp': jumps = t[3:]
            if t[0] == 'MessageBox':
                for j, arg in enumerate(t[:-1]):
                    if arg in ('IDYES','IDNO','IDOK','IDCANCEL') and j > 0 and t[j-1] != '/SD':
                        jumps.append(t[j+1])
            assert all(j in labels or j == '0' or re.fullmatch(r'[+-]\d+', j) for j in jumps), (name, t, jumps)
    ok('All explicit jump labels inside expanded functions resolve locally')
    declarations = set(re.findall(r'(?m)^Var\s+(\w+)', hooks))
    builtins = {'0','1','2','3','4','5','6','7','8','9','R0','R1','R2','R3','R4','R5','R6','R7','R8','R9','APPDATA','INSTDIR','TEMP','WINDIR','SYSDIR','PLUGINSDIR','EXEPATH'}
    for line in lines:
        t=nsis_tokens(line)
        for token in t:
            for var in re.findall(r'\$(?![${\\])([A-Za-z0-9_]+)', token):
                assert var in declarations|builtins, f'Unknown NSIS runtime variable {var}'
    ok('Every runtime variable used by expanded hooks is declared or a known NSIS variable')
    ps_files=[]
    for folder in ('scripts','src-tauri/windows','tests'):
        ps_files += list((root/folder).glob('*.ps1'))
    for file in ps_files:
        ps_brackets(file.read_text(encoding='utf-8-sig'))
    ok(f'Literal/comment-aware delimiter checks pass for {len(ps_files)} PowerShell files (not parser execution)')
    package=json.loads((root/'package.json').read_text())
    tauri=json.loads((root/'src-tauri/tauri.conf.json').read_text())
    cargo=re.search(r'^version\s*=\s*"([^"]+)"', (root/'src-tauri/Cargo.toml').read_text(), re.M)[1]
    assert package['version'] == tauri['version'] == cargo == '0.7.22'
    ok('Package, Tauri and Rust version metadata agree on 0.7.22')
    build=(root/'scripts/build-windows.ps1').read_text()
    assert '$nativeExit = $LASTEXITCODE' in build
    assert "'tests\\installer-safety.tests.ps1'" in build
    assert 'Assert-MdBuiltExe $setup' in build
    assert 'Copy-Item' in build and build.index('Assert-MdBuiltExe $setup') < build.index('# Publish')
    ok('Build contains native exit-code checks, preflight, required current Setup and deferred publishing')
    assert not (root/'src-tauri/windows/legacy-upgrade-cleanup.ps1').exists()
    ok('Obsolete separately-maintained legacy PowerShell helper has been removed')
    print(f'Source checks completed: {len(checks)}. Windows runtime/compiler testing remains separate.')

if __name__ == '__main__':
    main()
