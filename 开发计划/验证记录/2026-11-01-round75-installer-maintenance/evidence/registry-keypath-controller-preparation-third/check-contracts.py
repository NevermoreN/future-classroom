"""Pure controller contracts only: no SSH, PowerShell, MSI or Windows operation."""
import ast, base64, copy, gzip, json, pathlib, re, runpy, sys
sys.dont_write_bytecode=True
D=pathlib.Path(__file__).parent
m=runpy.run_path(str(D/'run-registry-fixture-third.py'))
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
check('bounded_remaining_original120s_forwarded', '-AvailableMilliseconds $c.available' in m['INVOKE'] and 'remaining()-20' in (D/'run-registry-fixture-third.py').read_text())
check('original_reserved_uninstall_budget',"$msiexec ('/i '" in ps and ') 35 55' in ps and ') 45 10' in ps)
check('exact_product_finally_no_foreign_uninstall',"if($report.beforeAbsent -and $report.installAttempted)" in ps and "'/x '+$product+' /qn /norestart" in ps)
check('no_registry_cache_or_SSH_authorization_mutation',not any(t in ps for t in ('Remove-ItemProperty','Remove-Registry','Stop-Service','sshd','authorized_keys','Windows\\Installer')))
check('only_exact_root_removed_after_receipt',ps.count('Remove-Item')==1 and 'Remove-Item -LiteralPath $Root -Recurse -Force' in ps and 'Safe-CleanupReport $report' in ps)
# Regress the actual first observation, not a fabricated success receipt.
observed=json.loads((D.parent/'registry-failed-first-remote-originals/Before.json').read_bytes())['queries'][0]
check('actual_first_UNKNOWN1605_capacity_input_retained',observed['componentState']==-1 and observed['productVersionCode']==1605 and observed['pathReturnedLength']==4096 and observed['versionReturnedLength']==4096)
def overflow(state,code,path_len,version_len):
    return state==-3 or code==234 or (state==3 and path_len>=4096) or (code==0 and version_len>=4096)
check('actual_first_unknown_not_false_overflow',not overflow(observed['componentState'],observed['productVersionCode'],observed['pathReturnedLength'],observed['versionReturnedLength']))
for label,values in [('component_MOREDATA',(-3,1605,0,0)),('product_MOREDATA',(-1,234,0,0)),('local_oversize',(3,1605,4096,4096)),('success_version_oversize',(-1,0,4096,4096))]:
    check('reject_'+label,overflow(*values))
check('success_max_bounded',not overflow(3,0,4095,4095))
query=(D/'query-registry-x86.ps1').read_text()
old=(D.parent/'registry-keypath-fixture-preparation-first/query-registry-x86.ps1').read_text()
oldline="if ($pathCapacity -ge 4096 -or $versionCapacity -ge 4096 -or $state -eq -3 -or $code -eq 234) { throw 'BOUNDED_QUERY_OVERFLOW' }"
newline="if ($state -eq -3 -or $code -eq 234 -or ($state -eq 3 -and $pathCapacity -ge 4096) -or ($code -eq 0 -and $versionCapacity -ge 4096)) { throw 'BOUNDED_QUERY_OVERFLOW' }"
baseline=(D.parent/'registry-keypath-controller-preparation-second/query-registry-x86.ps1').read_text()
added='''    // PowerShell-to-managed string binding is not the native NULL boundary.
    // The managed wrapper chooses the literal NULL required for MACHINE (4).
    public static int ComponentPath(string product, string component, string sid,
        uint context, StringBuilder path, ref uint capacity) {
        return MsiGetComponentPathExW(product, component, context == 4 ? null : sid,
            context, path, ref capacity);
    }
    public static uint ProductInfo(string product, string sid, uint context,
        string property, StringBuilder value, ref uint capacity) {
        return MsiGetProductInfoExW(product, context == 4 ? null : sid,
            context, property, value, ref capacity);
    }
'''
expected=baseline.replace('    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]\n    public static extern uint MsiEnumRelatedProductsW', added+'    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]\n    public static extern uint MsiEnumRelatedProductsW').replace('[R75RegistryKeypathQuery]::MsiGetComponentPathExW(', '[R75RegistryKeypathQuery]::ComponentPath(').replace('[R75RegistryKeypathQuery]::MsiGetProductInfoExW(', '[R75RegistryKeypathQuery]::ProductInfo(')
check('query_only_managed_NULL_wrappers_and_two_calls_changed',query==expected)
check('native_NULL_choice_inside_CSharp_not_PS_binding',query.count('context == 4 ? null : sid')==2)
second_observed=json.loads((D.parent/'root-registry-keypath-windows-second-original-bytes/Before.json').read_bytes())
check('actual_second_machine_BADARG_not_relabelled_absent',second_observed['queries'][2]['context']==4 and second_observed['queries'][2]['componentState']==-2 and second_observed['queries'][2]['productVersionCode']==87)
check('all_contexts_and_strict_absence_statuses_preserved',query[query.index('    foreach ($entry in $report.queries)'):]==baseline[baseline.index('    foreach ($entry in $report.queries)'):])
check('all_actual_phase_absence_assertions_unchanged',query[query.index('    foreach ($entry in $report.queries)'):]==old[old.index('    foreach ($entry in $report.queries)'):])
check('Rows_explicit_Int64_same_original_bound',"[int64]$totalBytes=0" in ps and "$totalBytes += [int64]$row['bytes']" in ps and '$totalBytes -le 67108864' in ps and 'Measure-Object' not in ps)
check('outer_process_ExecutionPolicyBypass_retained','-ExecutionPolicy' in m['remote_command'](m['INVOKE'],config) and 'Bypass' in m['remote_command'](m['INVOKE'],config))
print(json.dumps(dict(status='PASS_PURE_CONTRACTS_NOT_WINDOWS',count=len(checks),checks=checks,powershellSyntaxRuntime='NOT_RUN_NO_HOST_POWERSHELL',windowsExecuted=False,msiExecuted=False),indent=2))
