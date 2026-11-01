from pathlib import Path
p=Path('/tmp/fc-round61-gui/worktree');e=Path('/tmp/fc-round61-gui/evidence')
paths=['scripts/windows-msvc-ci.ps1','scripts/check-msvc-gui-contract.ps1']
for f in paths:
 t=e/'bootstrap-ci-before-second'/f;t.parent.mkdir(parents=True,exist_ok=True);t.write_bytes((p/f).read_bytes())
f=p/paths[0];s=f.read_text()
a='''        if ($Command -eq 'rustc') {
            # Only the selected final product gets this tail. Dependency/test
            # flags stay CONSOLE6.01; actual product GUI/version is checked below.
            return ,(@('-Z','build-std',$Command) + $Arguments + @('--','-C','link-arg=/SUBSYSTEM:WINDOWS,6.01'))
        }
'''
assert a in s;s=s.replace(a,'')
a='function Test-MsvcExecutableHeader([byte[]]$Bytes,'
b='''function Get-ProductRustFlags([ValidateSet('Modern', 'Win7')][string]$Name = 'Modern') {
    if ($Name -eq 'Win7') { return '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01' }
    return (Get-MsvcBaseline $Name).rustflags
}
function Test-MsvcExecutableHeader([byte[]]$Bytes,'''
s=s.replace(a,b)
a='    rustflags = $BaselineConfiguration.rustflags; baseline = $Baseline; role = $BaselineConfiguration.role; tests = \'COMPILE_ONLY_NO_RUN\''
b='''    rustflags = $BaselineConfiguration.rustflags; product_rustflags = Get-ProductRustFlags $Baseline
    rustflags_scope = 'Workspace test/clippy default; product-compile uses product_rustflags only and restores the original flags.'
    baseline = $Baseline; role = $BaselineConfiguration.role; tests = 'COMPILE_ONLY_NO_RUN' '''.rstrip()
assert a in s;s=s.replace(a,b)
a="    $binary = Invoke-Logged 'product-compile' $cargo (Get-BaselineCargoArguments 'rustc' @('-vv','--release','--locked','--target',$Target,'-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json') $Baseline) 1200"
b='''    # Cargo appends environment RUSTFLAGS after cargo-rustc trailing flags.
    # The actual final product needs a scoped environment, not a conflicting
    # trailing /SUBSYSTEM argument. Dependencies may rebuild; budget is unchanged.
    $productRustFlags = Get-ProductRustFlags $Baseline
    $originalProductRustFlags = $env:RUSTFLAGS
    try {
        $env:RUSTFLAGS = $productRustFlags
        Write-CrtFlagState 'before-product' $productRustFlags
        $binary = Invoke-Logged 'product-compile' $cargo (Get-BaselineCargoArguments 'rustc' @('-vv','--release','--locked','--target',$Target,'-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json') $Baseline) 1200
    } finally {
        $env:RUSTFLAGS = $originalProductRustFlags
        Write-CrtFlagState 'after-product' $BaselineConfiguration.rustflags
    }'''
assert a in s;s=s.replace(a,b)
a='    source_sha = $ExpectedSourceSha; input_manifest_sha256 = $inputDigest; toolchain = $Toolchain; target = $Target; baseline = $Baseline'
b=a+'''\n    rustflags = $BaselineConfiguration.rustflags; product_rustflags = Get-ProductRustFlags $Baseline
    rustflags_scope = 'Workspace test/clippy default; product-compile uses product_rustflags only and restores the original flags.' '''.rstrip()
assert a in s;s=s.replace(a,b)
f.write_text(s)
f=p/paths[1];s=f.read_text()
s=s.replace("'Get-BaselineCargoArguments', 'Test-MsvcExecutableHeader'", "'Get-BaselineCargoArguments', 'Get-ProductRustFlags', 'Test-MsvcExecutableHeader'")
s=s.replace("Check 'normal rustc selects only product and appends Win7 GUI version to final target'", "Check 'normal rustc selects only product without a conflicting final subsystem override'")
s=s.replace("(@('-Z','build-std','rustc') + $win7Args + @('--','-C','link-arg=/SUBSYSTEM:WINDOWS,6.01'))", "(@('-Z','build-std','rustc') + $win7Args)")
a="    Check 'test build clippy metadata and fmt argument contracts remain unchanged' {"
b='''    Check 'product-only environment and finally restore are wired around the real command' {
        Require ((Get-ProductRustFlags Modern) -ceq '-C target-feature=+crt-static') 'Modern product flags changed.'
        Require ((Get-ProductRustFlags Win7) -ceq '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01') 'Win7 product environment must be GUI6.01.'
        Require ((Get-MsvcBaseline Win7).rustflags -ceq '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01') 'Win7 workspace flags must stay console.'
        $productScopes = @($ast.FindAll({ param($node)
            $node -is [Management.Automation.Language.TryStatementAst] -and
            $null -ne $node.Finally -and $node.Body.Extent.Text.Contains("Invoke-Logged 'product-compile'")
        }, $true))
        Require ($productScopes.Count -eq 1) 'Product command must have exactly one containing try/finally.'
        $body = [string]$productScopes[0].Body.Extent.Text
        $final = [string]$productScopes[0].Finally.Extent.Text
        Require ($body.IndexOf('$env:RUSTFLAGS = $productRustFlags') -ge 0 -and
            $body.IndexOf('$env:RUSTFLAGS = $productRustFlags') -lt $body.IndexOf("Write-CrtFlagState 'before-product'") -and
            $body.IndexOf("Write-CrtFlagState 'before-product'") -lt $body.IndexOf("Invoke-Logged 'product-compile'")) 'Product environment is not set and captured before the command.'
        Require ($final.IndexOf('$env:RUSTFLAGS = $originalProductRustFlags') -ge 0 -and
            $final.IndexOf('$env:RUSTFLAGS = $originalProductRustFlags') -lt $final.IndexOf("Write-CrtFlagState 'after-product'")) 'Finally must restore before capturing original flags.'
        Require ($TextSource.Contains('$originalProductRustFlags = $env:RUSTFLAGS')) 'Original product environment was not captured.'
        Require ($TextSource.Contains('product_rustflags = Get-ProductRustFlags $Baseline')) 'Metadata lost product-only flag exception.'
    }
''' + a
assert a in s;s=s.replace(a,b)
f.write_text(s)
