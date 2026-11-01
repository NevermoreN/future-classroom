from pathlib import Path
import hashlib,json,ast
P=Path(__file__).parent
def ident(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def put(p,o):
 with p.open('x') as f:json.dump(o,f,ensure_ascii=False,indent=2);f.write('\n')
first=json.loads((P/'windows-run-contract-first.json').read_bytes());src=P/'run-windows-probe.ps1';s=src.read_text()
checks={
'old_absence_claim_removed': '$base.CreateSubKey($expectedRegistry)' not in s and '$existing = $base.OpenSubKey' not in s,
'only_single_leaf_native_creation': 'RegCreateKeyExW(parent, leaf,' in s and 'RegOpenKeyExW(new IntPtr(unchecked((int)0x80000001)), "Software"' in s,
'actual_new_before_markers': s.index('if($created.Disposition -ne 1)')<s.index("$script:ownerKey.SetValue('ProbeNonce'"),
'original_returned_handle_used': '[Microsoft.Win32.RegistryKey]::FromHandle($script:ownerHandle' in s,
'existing_does_not_reach_owned_cleanup': 'if($script:ownerNew -and $script:markersWritten -and -not $script:unknown)' in s,
'marker_partial_retained': 'RETAINED_ACTUAL_NEW_ROOT_MARKERS_INCOMPLETE_NO_MSI_CLEANUP' in s,
'unknown_before_next_operation': s.index('Assert-OriginalsReturned',s.index('function Invoke-OwnedMsi'))<s.index('$operation=[Guid]',s.index('function Invoke-OwnedMsi')),
'missing_guardian_is_unknown': "UNKNOWN_GUARDIAN_NOT_RETAINED" in s,
'all_started_exception_unknown': "if($procStarted){$script:unknown=$true;" in s,
'delete_checks_structure_and_processes': 'Assert-OriginalsReturned\n                Assert-KnownOwnerTree\n                $base.DeleteSubKeyTree' in s,
'single_delete_site': s.count('DeleteSubKeyTree(')==1,
'no_kill_or_shared_service': all(w not in s for w in ['Stop-Process','taskkill','Kill()','Stop-Service']),
'budgets_not_extended': '$deadlineMillis=60000' in s and 'WaitForExit(5000)' in s,
}
assert all(checks.values()),checks
for old in first['staticProof'].values():
 if isinstance(old,dict) and 'path' in old:assert ident(Path(old['path']))==old
snap=P/'implementation-owner-second';snap.mkdir()
with (snap/src.name).open('xb') as f:f.write(src.read_bytes())
review={
'status':'PREPARED_SOURCE_REVIEW_ONLY_WINDOWS_POWERSHELL_NOT_PARSED_OR_EXECUTED',
'findingFirst':'The prior OpenSubKey-Absent then CreateSubKey sequence did not establish actual NEW ownership. Its finally could attempt product cleanup without successful ownership creation.',
'filesChanged':['run-windows-probe.ps1'],
'firstRunner':ident(P/'implementation-final/run-windows-probe.ps1'),'secondRunner':ident(snap/src.name),'patch':ident(P/'owner-second.patch'),
'localTextInvariants':checks,'localChecksScope':'Python evaluated these exact source-text predicates only. This is not PowerShell AST, Add-Type compilation, native registry disposition/marker/cleanup or installation execution.',
'closeAndFailureSemantics':[
'Open only existing HKCU32 Software, then native RegCreateKeyExW the single fixed random leaf. ERROR_SUCCESS plus disposition1 required before writing. Existing only has returned native handle disposed; no mark, product mutation or delete.',
'Nonce and original current SID are written via RegistryKey.FromHandle over the SAME returned owning SafeRegistryHandle. NEW disposition, parentOpen/Create/parentClose errors and each marker completion are recorded independently.',
'Failed marker write conservatively leaves the actually NEW root; original native handle is disposed in finally and handleDisposed reports disposal only, not a separate native-close completion receipt. It never removes or adopts an incompletely marked root.',
'Any exception after the original MSI starts sets unknown. Missing retained guardian, nonzero/unobserved End or incomplete physical exit prevents next MSI and final cleanup. Read-only query timeout also sets unknown.',
'Known cleanup requires actual NEW+both markers, all prior original msiexec and guardian actual exits, no unknown operation, correct marker types+nonce+SID, matching last operation control, bounded allowlisted known child/value structure, and exact own product absent. Then delete only the own leaf, close original key handles and reopen to observe absence.',
'The known-structure verification and path deletion are not an atomic content-CAS against a malicious concurrent same-SID writer. This fixture does not solve the production maintenance ownership protocol or withdrawn R56 CAS issue.'
],
'unchangedMsi':ident(P/'build-fifth/fixture.msi'),'unchangedGuardian':ident(P/'build-fifth/guardian.exe'),
'notRun':['Windows','PowerShell ParseInput','Add-Type C# compile','MSI install repair uninstall','native registry NEW/EXISTING tests','Cargo','SSH','CI'],
'pendingRootChecks':['Parse full second runner under real Windows PowerShell5.1 before explicit execution.','Verify Existing disposition leaves all preexisting values byteexact unchanged and does not start or clean MSI.','Verify actual NEW disposition/partial-marker failure retention/known-structure refusal and original handle cleanup with precise owned fixture.','Perform bounded direct /i /f /x and rollback feasibility only after independent review.']}
put(P/'owner-boundary-review-second.json',review)
contract=json.loads(json.dumps(first));contract['supersedesRunnerContractOnly']=ident(P/'windows-run-contract-first.json')
contract['filesToCopyToNewWindowsOwnedRoot'][-1]=ident(snap/src.name)
contract['entry']['script']='run-windows-probe.ps1 (immutable implementation-owner-second copy)'
contract['inputScopeGuards']=[x for x in contract['inputScopeGuards'] if not x.startswith('Existing same-nonce')]+[
'Only native RegCreateKeyExW ERROR_SUCCESS+REG_CREATED_NEW_KEY disposition authorizes same-handle nonce/SID marking. REG_OPENED_EXISTING_KEY never authorizes write/delete/MSI.',
'Marker partial failure reports actual NEW and per-marker progress, closes original owner handles, conservatively retains the root and forbids product cleanup.',
'All later operations and cleanup require all prior exact original MSI/guardian physical exits and known End success; unknown or uncaptured guardian prevents another MSI.',
'Delete requires exact original marker kinds/nonce/SID and bounded known full subtree; no adversarial same-SID atomic content-CAS guarantee.']
contract['observationsAndOutcomes']['cleanup']=review['closeAndFailureSemantics'][4]
contract['observationsAndOutcomes']['registryOwnerReport']='registryCreation contains raw parentOpenError/createError/parentCloseError/disposition plus actualNew/nonceWritten/sidWritten/markersComplete/handleDisposed; no prior Absent observation substituted for NEW.'
contract['sourceReview']=ident(P/'owner-boundary-review-second.json')
contract['pendingBeforeActualRun'].append('Second runner native PInvoke, Add-Type and ownership/known-tree cleanup have NOT_RUN/NOT_PARSED on Windows. Same-SID hostile replacement CAS is outside this feasibility fixture.')
put(P/'windows-run-contract-second.json',contract)
freeze={'status':'FROZEN_SECOND_RUNNER_SOURCE_ONLY_WINDOWS_PENDING_REVIEW','scope':'Minimal evidence-local runner-only registry ownership and conservative physical-cleanup fix; MSI, guardian and first freeze unchanged.',
'firstFreeze':ident(P/'feasibility-freeze-first.json'),'secondImmutableRunner':ident(snap/src.name),'runnerActiveEqualsImmutable':src.read_bytes()==(snap/src.name).read_bytes(),
'review':ident(P/'owner-boundary-review-second.json'),'contract':ident(P/'windows-run-contract-second.json'),'patch':ident(P/'owner-second.patch'),'unchangedMsi':ident(P/'build-fifth/fixture.msi'),'unchangedGuardian':ident(P/'build-fifth/guardian.exe'),
'freezeHelper':ident(Path(__file__)),'windowsSyntax':'NOT_PARSED_NO_LOCAL_PWSH','windowsExecution':'NOT_RUN','mainR61RustCargo':'NOT_TOUCHED','furtherWrites':'Stop writing this R60 evidence root after this freeze; root reviews and archives later.'}
put(P/'feasibility-freeze-second.json',freeze)
print(json.dumps({'freeze':ident(P/'feasibility-freeze-second.json'),'runner':ident(src),'contract':ident(P/'windows-run-contract-second.json'),'review':ident(P/'owner-boundary-review-second.json'),'textChecks':len(checks)},ensure_ascii=False))
