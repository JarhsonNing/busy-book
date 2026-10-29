param([string]$Program, [string]$Workspace, [string]$Options, [switch]$HostTerminal)
$ErrorActionPreference = 'Stop'
if ($HostTerminal) {
    # The Rust owner attaches its Windows job before releasing this gate.
    if ([Console]::ReadLine() -ne 'start') { exit 1 }
    function Quote-Argument([string]$value) { '"' + $value + '"' }
    $terminalArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Quote-Argument $PSCommandPath), '-Program', (Quote-Argument $Program), '-Workspace', (Quote-Argument $Workspace))
    if ($Options) { $terminalArgs += @('-Options', (Quote-Argument $Options)) }
    # This is the interactive CLI the user opens, so it needs a visible console.
    $terminal = Start-Process -FilePath 'powershell.exe' -ArgumentList $terminalArgs -WorkingDirectory $Workspace -PassThru
    [Console]::WriteLine('ready')
    $terminal.WaitForExit()
    exit $terminal.ExitCode
}
Set-Location -LiteralPath $Workspace
$launchOptions = if ($Options) { [IO.File]::ReadAllText($Options) | ConvertFrom-Json } else { $null }
if ($launchOptions.login) {
    & $Program --cwd $Workspace login
} else {
    $qoderArgs = @('--cwd', $Workspace, '--strict-mcp-config', '--mcp-config', (Join-Path $Workspace '.mcp.json'))
    if ($launchOptions.model) { $qoderArgs += @('--model', $launchOptions.model) }
    & $Program @qoderArgs
}
if ($LASTEXITCODE -ne 0) {
    Write-Host 'Qoder 已退出。请检查登录或网络后，在淘书工坊中重新打开。'
    Read-Host '按 Enter 关闭窗口'
}
exit $LASTEXITCODE
