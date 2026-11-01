$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$observations=@()
foreach($literal in @('127.0.0.1','::1')) {
    $ip=[Net.IPAddress]::Parse($literal)
    $listener=$null
    $client=$null
    $row=@{address=$literal;family=$ip.AddressFamily.ToString();status='NOT_RUN';listenerStopped=$false;clientDisposed=$false;connectBudgetMilliseconds=1000}
    try {
        $listener=[Net.Sockets.TcpListener]::new($ip,0)
        $listener.ExclusiveAddressUse=$true
        if($ip.AddressFamily-eq [Net.Sockets.AddressFamily]::InterNetworkV6){$listener.Server.DualMode=$false}
        $listener.Start(4)
        $row.localEndpoint=$listener.LocalEndpoint.ToString()
        $client=[Net.Sockets.TcpClient]::new($ip.AddressFamily)
        $pending=$client.BeginConnect($ip,$listener.LocalEndpoint.Port,$null,$null)
        try {
            if(!$pending.AsyncWaitHandle.WaitOne(1000)){throw 'owned loopback connect observation exceeded its original1000ms'}
            $client.EndConnect($pending)
        } finally {$pending.AsyncWaitHandle.Close()}
        $row.status='CONNECTED'
        $row.connected=$client.Connected
        $row.actualClientLocal=$client.Client.LocalEndPoint.ToString()
        $row.actualClientRemote=$client.Client.RemoteEndPoint.ToString()
    } catch {
        $errorValue=$_.Exception.GetBaseException()
        $row.status='FAILED'
        $row.errorType=$errorValue.GetType().FullName
        $row.errorMessage=$errorValue.Message
        if($errorValue-is [Net.Sockets.SocketException]){$row.nativeErrorCode=$errorValue.NativeErrorCode;$row.socketErrorCode=$errorValue.SocketErrorCode.ToString()}
    } finally {
        if($null-ne $client){$client.Dispose();$row.clientDisposed=$true}
        if($null-ne $listener){$listener.Stop();$row.listenerStopped=$true}
    }
    $observations+=$row
}
@{status='DIAGNOSTIC_COMPLETED';cases=$observations;scope='Owned transient .NET loopback sockets under the same SSH user; no native TLS product validation, network policy or firewall change.'}|ConvertTo-Json -Depth 6 -Compress
