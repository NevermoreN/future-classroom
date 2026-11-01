"""Pure controller contracts only: no SSH, PowerShell, MSI or Windows operation."""
import ast, base64, copy, gzip, json, pathlib, re, runpy, sys
sys.dont_write_bytecode=True
D=pathlib.Path(__file__).parent
m=runpy.run_path(str(D/'run-registry-fixture-first.py'))
checks=[]
def check(name, condition):
    assert condition,name
    checks.append(name)
def rejected(name, fn):
    try: fn()
    except (AssertionError,ValueError,TypeError): checks.append(name)
    else:raise AssertionError('accepted '+name)
for path in D.glob('*.py'):ast.parse(path.read_bytes())
check('AST_all_prepared_python',True)
m['verify_sources']();check('actual_original_static_report_and_MSI_spec_query_bound',True)
r=dict(identityVerified=True,beforeAbsent=True,installAttempted=True,afterAbsent=True,measurementPassed=True,commands=[dict(name='uninstall',actualExit=True,pipesComplete=True,timedOut=False,exitCode=0)])
check('formal_exit0_absence_owner_allows_directory_cleanup',m['eligible_cleanup'](r))
r1605=copy.deepcopy(r);r1605['commands'][0]['exitCode']=1605
check('absent_product1605_still_requires_after_query',m['eligible_cleanup'](r1605))
for key in ('identityVerified','beforeAbsent','installAttempted','afterAbsent'):
    changed=copy.deepcopy(r);changed[key]=False
    check('reject_'+key,not m['eligible_cleanup'](changed))
for key,value in [('actualExit',False),('pipesComplete',False),('timedOut',True),('exitCode',1618)]:
    changed=copy.deepcopy(r);changed['commands'][0][key]=value
    check('reject_'+key,not m['eligible_cleanup'](changed))
changed=copy.deepcopy(r);changed['commands']=[]
check('reject_missing_formal_uninstall',not m['eligible_cleanup'](changed))
changed=copy.deepcopy(r);changed['commands']*=2
check('reject_duplicate_formal_uninstall',not m['eligible_cleanup'](changed))
changed=copy.deepcopy(r);changed['measurementPassed']=False
check('measurement_FAIL_can_cleanup_only_returned_exact_fixture',m['eligible_cleanup'](changed))
root=r'C:\Users\Elior\AppData\Local\Temp\FutureClassroom-r75-registry-keypath-'+m['NONCE']
rows=[dict(path=p,bytes=1,sha256='a'*64) for p in ('.owner',*m['INPUTS'],'controller.ps1','controller-report.json')]
s=dict(status='SNAPSHOT_ONLY',nonce=m['NONCE'],root=root,files=rows)
check('snapshot_closed_original_members',m['validate_snapshot'](s,root)==rows)
for path in ('../other','/absolute','C:/other','unknown.key','arbitrary/empty','.owner/../foo','query-temp-Before/../../foo','query-temp-Before\\other'):
    c=copy.deepcopy(s);c['files'].append(dict(path=path,bytes=1,sha256='a'*64))
    rejected('reject_path_'+path,lambda:m['validate_snapshot'](c,root))
c=copy.deepcopy(s);c['files']*=2
rejected('reject_duplicate_inventory',lambda:m['validate_snapshot'](c,root))
c=copy.deepcopy(s);c['files'][0]['bytes']=16777217
rejected('reject_oversize_inventory',lambda:m['validate_snapshot'](c,root))
c=copy.deepcopy(s);c['files'][0]['sha256']='x'*64
rejected('reject_nonhex_SHA',lambda:m['validate_snapshot'](c,root))
config=dict(root=root+'A'*160,mode='Run',controllerSha256='f'*64,relative='',proof='')
cmd=m['remote_command'](m['INVOKE'],config)
check('SSH_fixed_key_path_only_no_key_read',cmd[:4]==['ssh','-i',m['IDENTITY'],'-o'])
check('bounded_encoded_command',len(cmd[-1])<=7500)
decoder=base64.b64decode(cmd[-1]).decode('utf-16-le')
body=gzip.decompress(base64.b64decode(re.search(r"FromBase64String\('([^']+)'\)",decoder)[1])).decode()
packed=re.search(r"FromBase64String\('([^']+)'\)",body)[1]
check('lossless_config_no_shell_interpolation',json.loads(base64.b64decode(packed))==config and body.endswith(m['INVOKE']))
ps=(D/'controller.ps1').read_text()
check('single_clock_origin_no_restart',ps.count('[Diagnostics.Stopwatch]::StartNew()')==1 and '$AvailableMilliseconds-$clock.ElapsedMilliseconds' in ps)
check('bounded_remaining_original120s_forwarded', '-AvailableMilliseconds $c.available' in m['INVOKE'] and 'remaining()-20' in (D/'run-registry-fixture-first.py').read_text())
check('original_reserved_uninstall_budget',"$msiexec ('/i '" in ps and ') 35 55' in ps and ') 45 10' in ps)
check('exact_product_finally_no_foreign_uninstall',"if($report.beforeAbsent -and $report.installAttempted)" in ps and "'/x '+$product+' /qn /norestart" in ps)
check('no_registry_cache_or_SSH_authorization_mutation',not any(t in ps for t in ('Remove-ItemProperty','Remove-Registry','Stop-Service','sshd','authorized_keys','Windows\\Installer')))
check('only_exact_root_removed_after_receipt',ps.count('Remove-Item')==1 and 'Remove-Item -LiteralPath $Root -Recurse -Force' in ps and 'Safe-CleanupReport $report' in ps)
print(json.dumps(dict(status='PASS_PURE_CONTRACTS_NOT_WINDOWS',count=len(checks),checks=checks,powershellSyntaxRuntime='NOT_RUN_NO_HOST_POWERSHELL',windowsExecuted=False,msiExecuted=False),indent=2))
