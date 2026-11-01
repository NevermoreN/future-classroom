from pathlib import Path
import hashlib
import json
import os
import subprocess
import xml.etree.ElementTree as ET

EV = Path(__file__).resolve().parent
OUT = EV / 'tool-smoke-first'
OUT.mkdir()
(OUT / '.owner').write_text('future-classroom-owned-r53-msi-tool-smoke/v1\n')
payload = OUT / '中文输入'
payload.mkdir()
(payload / 'payload.txt').write_text('SYNTHETIC MSI TOOLCHAIN ONLY; NO PRODUCT OR SCHOOL DATA\n')
source = '''<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
<Product Id="F6D35070-570B-49EC-A9B6-C419F067A689" Name="FutureClassroom tool smoke only" Language="1033" Version="0.1.0" Manufacturer="FutureClassroom" UpgradeCode="33E2681A-F337-4770-A5F5-20593CF55618">
<Package InstallerVersion="500" Compressed="yes" InstallScope="perUser"/>
<Media Id="1" Cabinet="payload.cab" EmbedCab="yes"/>
<MajorUpgrade AllowDowngrades="no" AllowSameVersionUpgrades="no" DowngradeErrorMessage="A newer version is installed."/>
<Condition Message="This candidate is per-user only.">NOT ALLUSERS</Condition>
<Condition Message="Windows 7 or later is required.">VersionNT &gt;= 601</Condition>
<Property Id="MSIRESTARTMANAGERCONTROL" Value="DisableShutdown"/>
<Directory Id="TARGETDIR" Name="SourceDir"><Directory Id="LocalAppDataFolder"><Directory Id="Programs" Name="Programs"><Directory Id="INSTALLDIR" Name="FutureClassroomToolSmoke"><Component Id="Payload" Guid="C2661F23-D299-4D93-9207-D39D82C80C23" Win64="no"><File Id="PayloadFile" Name="payload.txt" Source="SOURCE" KeyPath="yes"/></Component></Directory></Directory></Directory></Directory>
<Feature Id="Main" Level="1"><ComponentRef Id="Payload"/></Feature>
</Product></Wix>'''
tree = ET.fromstring(source)
tree.find('.//{http://schemas.microsoft.com/wix/2006/wi}File').set('Source', str(payload / 'payload.txt'))
ET.ElementTree(tree).write(OUT / 'smoke.wxs', encoding='utf-8', xml_declaration=True, default_namespace='http://schemas.microsoft.com/wix/2006/wi')
tools = Path('/tmp/fc-round53-msi-tools/root/usr/bin')
env = dict(os.environ)
env['LD_LIBRARY_PATH'] = '/tmp/fc-round53-msi-tools/root/usr/lib/x86_64-linux-gnu'
report = {'status': 'RUNNING', 'scope': 'Actual native Linux MSI compiler/database/extractor fixture only; no product EXE or Windows Installer execution.', 'commands': []}


def run(name, args):
    process = subprocess.run(args, cwd=OUT, env=env, capture_output=True, timeout=45)
    for label, data in [('stdout', process.stdout), ('stderr', process.stderr)]:
        (OUT / (name + '.' + label)).write_bytes(data)
    report['commands'].append({'name': name, 'args': [str(x) for x in args], 'exitCode': process.returncode})
    assert process.returncode == 0, name
    return process.stdout


try:
    msi = OUT / 'smoke.msi'
    run('compile', [tools / 'wixl', '-a', 'x86', '-o', msi, OUT / 'smoke.wxs'])
    assert not (OUT / 'compile.stderr').read_bytes()
    run('summary', [tools / 'msiinfo', 'suminfo', msi])
    tables = run('tables', [tools / 'msiinfo', 'tables', msi]).decode().splitlines()
    for table in ['Property', 'Directory', 'Component', 'File', 'Upgrade', 'LaunchCondition', 'InstallExecuteSequence', 'Media']:
        assert table in tables, table
        run('table-' + table, [tools / 'msiinfo', 'export', msi, table])
    assert not {'CustomAction', 'Registry', 'RemoveRegistry', 'RemoveFile', 'ServiceInstall', 'ServiceControl'} & set(tables)
    extracted = OUT / 'extracted'
    extracted.mkdir()
    run('extract', [tools / 'msiextract', '-C', extracted, msi])
    files = [p for p in extracted.rglob('*') if p.is_file()]
    assert len(files) == 1 and files[0].read_bytes() == (payload / 'payload.txt').read_bytes()
    report.update(status='PASS_ACTUAL_MSI_TOOLCHAIN_FIXTURE_ONLY', msiBytes=msi.stat().st_size,
                  msiSha256=hashlib.sha256(msi.read_bytes()).hexdigest(), extractedExact=True)
except Exception as error:
    report.update(status='FAIL', errorKind=type(error).__name__, failedStep=str(error))
finally:
    (OUT / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({'status': report['status'], 'commands': len(report['commands']), 'errorKind': report.get('errorKind'), 'step': report.get('failedStep')}))
raise SystemExit(0 if report['status'].startswith('PASS') else 1)
